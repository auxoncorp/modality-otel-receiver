mod auth;
mod config;
mod opts;
mod otlp;

use std::{
    collections::{
        hash_map::{DefaultHasher, Entry},
        BTreeMap, HashMap,
    },
    hash::Hasher,
};

use clap::Parser;
use config::OtelReceiverConfig;
use modality_api::{AttrVal, TimelineId};
use modality_ingest_client::{
    dynamic::DynamicIngestClient, IngestClient, ReadyState, UnauthenticatedState,
};
use modality_ingest_protocol::InternedAttrKey;
use opts::Opts;
use otlp::opentelemetry::proto::{
    common::v1::{any_value, AnyValue, KeyValue},
    trace::v1::ResourceSpans,
};
use tokio::sync::mpsc::Receiver;
use tracing::{error, info};
use uuid::Uuid;

type DynError = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() {
    if let Err(e) = do_main().await {
        error!(error = e.as_ref());
    }
}

async fn do_main() -> Result<(), DynError> {
    let opts = Opts::parse();

    tracing_subscriber::fmt::init();

    let cfg = OtelReceiverConfig::load_merge_with_opts(opts)?;

    let (message_tx, message_rx) = tokio::sync::mpsc::channel(1024);

    let addr = cfg.otlp_addr();
    let otlp_server = tokio::spawn(async move {
        info!("OTLP/gRPC listening at http://{addr}");
        otlp::run_receiver(addr, message_tx).await
    });

    let client = IngestClient::<UnauthenticatedState>::connect(
        &cfg.protocol_parent_url()?,
        cfg.ingest.allow_insecure_tls,
    )
    .await?;

    let client = client.authenticate(cfg.resolve_auth()?.to_vec()).await?;
    let ingest = tokio::spawn(async move {
        ResourceSender::send_loop(client, &cfg.plugin.run_id, message_rx).await
    });

    ingest.await??;
    otlp_server.await??;

    Ok(())
}

struct ResourceSender {
    run_id: Uuid,
    client: DynamicIngestClient,
    keys: Keys,
}

mod keys {
    pub mod timeline {
        pub const RUN_ID: &str = "timeline.run_id";
        pub const NAME: &str = "timeline.name";

        pub mod trace {
            pub const ID: &str = "timeline.trace.id";
        }

        pub mod service {
            pub const NAMESPACE: &str = "timeline.service.namespace";
            pub const NAME: &str = "timeline.service.name";
        }

        pub mod span {
            pub const ID: &str = "timeline.span.id";
            pub const NAME: &str = "timeline.span.name";
        }
    }

    pub mod event {
        pub const NAME: &str = "event.name";
        pub const TIMESTAMP: &str = "event.timestamp";
        pub const NONCE: &str = "event.nonce";

        pub mod trace {
            pub const ID: &str = "event.trace.id";
        }

        pub mod span {
            pub const ID: &str = "event.span.id";
            pub const NAME: &str = "event.span.name";
            pub const PARENT_SPAN_ID: &str = "event.span.parent_span_id";
        }

        pub mod interaction {
            pub const REMOTE_TIMELINE_ID: &str = "event.interaction.remote_timeline_id";
            pub const REMOTE_NONCE: &str = "event.interaction.remote_nonce";
        }
    }
}

struct Keys {
    interned_keys: HashMap<String, InternedAttrKey>,
}

impl Keys {
    async fn new() -> Result<Self, DynError> {
        Ok(Self {
            interned_keys: HashMap::<String, InternedAttrKey>::new(),
        })
    }

    async fn intern_key<'a>(
        &mut self,
        key: String,
        client: &mut DynamicIngestClient,
    ) -> Result<InternedAttrKey, DynError> {
        Self::intern_key_inner(key, &mut self.interned_keys, client).await
    }

    async fn intern_key_inner<'a>(
        key: String,
        map: &mut HashMap<String, InternedAttrKey>,
        client: &mut DynamicIngestClient,
    ) -> Result<InternedAttrKey, DynError> {
        match map.entry(key.clone()) {
            Entry::Occupied(occupado) => Ok(*occupado.get()),
            Entry::Vacant(desoccupado) => {
                let interned = client.declare_attr_key(key).await?;
                Ok(*desoccupado.insert(interned))
            }
        }
    }
}

impl ResourceSender {
    async fn send_loop(
        client: IngestClient<ReadyState>,
        run_id: &Option<Uuid>,
        mut message_rx: Receiver<Vec<ResourceSpans>>,
    ) -> Result<(), DynError> {
        let client = DynamicIngestClient::from(client);
        let keys = Keys::new().await?;
        let mut sender = ResourceSender::new(client, keys, run_id);

        while let Some(message) = message_rx.recv().await {
            let tls = resource_spans_to_timelines(&sender.run_id, message);
            sender.send_timelines(tls).await?;
        }

        Ok(())
    }

    fn new(client: DynamicIngestClient, keys: Keys, run_id: &Option<Uuid>) -> Self {
        Self {
            run_id: match run_id {
                Some(uuid) => *uuid,
                None => Uuid::new_v4(),
            },
            client,
            keys,
        }
    }

    async fn interned_key<'a>(&mut self, key: String) -> Result<InternedAttrKey, DynError> {
        self.keys.intern_key(key, &mut self.client).await
    }

    async fn prepare_attr_map(
        &mut self,
        attrs: AttrMap,
    ) -> Result<Vec<(InternedAttrKey, AttrVal)>, DynError> {
        let mut res = vec![];
        for (k, v) in attrs.0.into_iter() {
            res.push((self.interned_key(k).await?, v));
        }

        Ok(res)
    }

    async fn send_timelines(&mut self, tls: Vec<SendableTimeline>) -> Result<(), DynError> {
        for tl in tls.into_iter() {
            self.client.open_timeline(tl.id).await?;
            let tl_attrs = self.prepare_attr_map(tl.attrs).await?;
            self.client.timeline_metadata(tl_attrs).await?;

            for (coarse_ordering, events) in tl.events.into_iter() {
                for (fine_ordering, attrs) in events.into_iter().enumerate() {
                    let ordering = (coarse_ordering as u128) << 4 | (fine_ordering as u128);
                    let ev_attrs = self.prepare_attr_map(attrs).await?;
                    self.client.event(ordering, ev_attrs).await?;
                }
            }
        }

        Ok(())
    }
}

struct SendableTimeline {
    id: TimelineId,
    attrs: AttrMap,
    // coarse ordering (timestamp) -> attrmaps at that timestamp
    events: BTreeMap<u64, Vec<AttrMap>>,
}

impl SendableTimeline {
    fn new(id: TimelineId) -> Self {
        Self {
            id,
            attrs: AttrMap::default(),
            events: BTreeMap::default(),
        }
    }

    fn insert_event(&mut self, ordering: u64, attrs: AttrMap) {
        self.events.entry(ordering).or_default().push(attrs);
    }
}

#[derive(Default, Clone, Debug)]
struct AttrMap(pub HashMap<String, AttrVal>);

impl AttrMap {
    fn insert(&mut self, key: impl Into<String>, val: impl Into<AttrVal>) {
        self.0.insert(key.into(), val.into());
    }

    fn extend(&mut self, other: &AttrMap) {
        for (k, v) in other.0.iter() {
            self.0.insert(k.clone(), v.clone());
        }
    }

    fn get(&self, key: &str) -> Option<&AttrVal> {
        self.0.get(key)
    }
}

fn resource_spans_to_timelines(
    instance_id: &Uuid,
    resource_spans_list: Vec<ResourceSpans>,
) -> Vec<SendableTimeline> {
    let mut timelines = vec![];

    // Events to add to other timelines, after the initial pass
    let mut timeline_events_to_add: HashMap<TimelineId, Vec<(u64, AttrMap)>> = HashMap::new();

    for resource_spans in resource_spans_list.into_iter() {
        // These will go on each timeline
        let mut resource_attrs = AttrMap::default();
        if let Some(resource) = resource_spans.resource {
            otlp_kvs_to_modality(
                vec!["timeline".to_string()],
                resource.attributes,
                &mut resource_attrs,
            );
        }

        for scope_span in resource_spans.scope_spans.into_iter() {
            // scope attrs will go on the timeline
            let mut scope_attrs = AttrMap::default();
            if let Some(instrumentation_scope) = scope_span.scope {
                otlp_kvs_to_modality(
                    vec!["timeline".to_string(), "scope".to_string()],
                    instrumentation_scope.attributes,
                    &mut scope_attrs,
                );
            }

            for span in scope_span.spans.into_iter() {
                let timeline_id = semantic_timeline_id(&span.trace_id, &span.span_id);
                let mut tl = SendableTimeline::new(timeline_id);

                // timeline attrs
                tl.attrs.extend(&resource_attrs);
                tl.attrs.extend(&scope_attrs);
                tl.attrs
                    .insert(keys::timeline::RUN_ID, instance_id.to_string());
                tl.attrs
                    .insert(keys::timeline::trace::ID, hex::encode(&span.trace_id));
                tl.attrs
                    .insert(keys::timeline::span::ID, hex::encode(&span.span_id));
                tl.attrs
                    .insert(keys::timeline::span::NAME, span.name.clone());
                tl.attrs.insert(
                    keys::timeline::NAME,
                    compute_timeline_name(&resource_attrs, &span.name, timeline_id),
                );

                // emit a start event
                let mut start_event = AttrMap::default();
                start_event.insert(keys::event::trace::ID, hex::encode(&span.trace_id));
                start_event.insert(keys::event::span::ID, hex::encode(&span.span_id));
                start_event.insert(keys::event::span::NAME, AttrVal::from(span.name.clone()));

                let start_timestamp = AttrVal::Timestamp(span.start_time_unix_nano.into());
                start_event.insert(keys::event::TIMESTAMP, start_timestamp.clone());

                start_event.insert(keys::event::NAME, AttrVal::from(span.name.clone()));
                if !span.parent_span_id.is_empty() {
                    start_event.insert(
                        keys::event::span::PARENT_SPAN_ID,
                        hex::encode(&span.parent_span_id),
                    );
                }

                let parent_timeline_id = if !span.parent_span_id.is_empty() {
                    Some(semantic_timeline_id(&span.trace_id, &span.parent_span_id))
                } else {
                    None
                };
                if let Some(parent_timeline_id) = parent_timeline_id {
                    start_event.insert(
                        keys::event::interaction::REMOTE_TIMELINE_ID,
                        AttrVal::TimelineId(Box::new(parent_timeline_id)),
                    );
                    let fork_nonce =
                        fork_nonce(&span.trace_id, &span.parent_span_id, &span.span_id);
                    start_event.insert(keys::event::interaction::REMOTE_NONCE, fork_nonce);

                    let mut parent_fork_event = AttrMap::default();
                    parent_fork_event.insert(keys::event::NAME, format!("fork_{}", span.name));
                    parent_fork_event.insert(keys::event::NONCE, fork_nonce);
                    parent_fork_event.insert(keys::event::TIMESTAMP, start_timestamp.clone());
                    timeline_events_to_add
                        .entry(parent_timeline_id)
                        .or_default()
                        .push((span.start_time_unix_nano, parent_fork_event));
                }

                start_event.insert(
                    keys::event::TIMESTAMP,
                    AttrVal::Timestamp(span.start_time_unix_nano.into()),
                );

                otlp_kvs_to_modality(vec!["event".to_string()], span.attributes, &mut start_event);
                tl.insert_event(span.start_time_unix_nano, start_event);

                for event in span.events.into_iter() {
                    // emit individual events

                    let mut event_attrs = AttrMap::default();
                    event_attrs.insert(keys::event::NAME, event.name);
                    event_attrs.insert(
                        keys::event::TIMESTAMP,
                        AttrVal::Timestamp(event.time_unix_nano.into()),
                    );

                    otlp_kvs_to_modality(
                        vec!["event".to_string()],
                        event.attributes,
                        &mut event_attrs,
                    );

                    tl.insert_event(event.time_unix_nano, event_attrs);
                }

                let mut end_event = AttrMap::default();
                end_event.insert(
                    keys::event::NAME,
                    AttrVal::from(format!("end_{}", span.name)),
                );
                let end_timestamp = AttrVal::Timestamp(span.end_time_unix_nano.into());
                end_event.insert(keys::event::TIMESTAMP, end_timestamp.clone());

                if let Some(parent_timeline_id) = parent_timeline_id {
                    let join_nonce =
                        join_nonce(&span.trace_id, &span.parent_span_id, &span.span_id);
                    end_event.insert(keys::event::NONCE, join_nonce);

                    let mut parent_join_event = AttrMap::default();
                    parent_join_event.insert(keys::event::NAME, format!("join_{}", span.name));
                    parent_join_event.insert(keys::event::interaction::REMOTE_NONCE, join_nonce);
                    parent_join_event.insert(
                        keys::event::interaction::REMOTE_TIMELINE_ID,
                        AttrVal::TimelineId(Box::new(timeline_id)),
                    );
                    parent_join_event.insert(keys::event::TIMESTAMP, end_timestamp.clone());
                    timeline_events_to_add
                        .entry(parent_timeline_id)
                        .or_default()
                        .push((span.end_time_unix_nano, parent_join_event));
                } 

                tl.insert_event(span.end_time_unix_nano, end_event);
                timelines.push(tl);
            }
        }
    }

    // splice 'timeline_events_to_add' into the timelines themselves
    for (tl_id, events) in timeline_events_to_add.into_iter() {
        if let Some(tl) = timelines.iter_mut().find(|tl| tl.id == tl_id) {
            for (ordering, attrs) in events.into_iter() {
                tl.insert_event(ordering, attrs);
            }
        } else {
            let mut tl = SendableTimeline::new(tl_id);
            for (ordering, attrs) in events.into_iter() {
                tl.insert_event(ordering, attrs);
            }
            timelines.push(tl);
        }
    }

    timelines
}

fn compute_timeline_name(
    resource_attrs: &AttrMap,
    span_name: &str,
    timeline_id: TimelineId,
) -> String {
    let mut timeline_name_components = vec![];
    // The first two should be filled out by the otlp_kvs_to_modality call for the resource
    if let Some(ns) = resource_attrs.get(keys::timeline::service::NAMESPACE) {
        timeline_name_components.push(ns.to_string());
    }
    if let Some(name) = resource_attrs.get(keys::timeline::service::NAME) {
        timeline_name_components.push(name.to_string());
    }

    timeline_name_components.push(span_name.to_string());

    if timeline_name_components.is_empty() {
        timeline_name_components.push(timeline_id.to_string());
    }
    timeline_name_components.join(".")
}

const NAMESPACE_OTEL: Uuid = Uuid::from_bytes([
    0x34, 0xdc, 0x16, 0x00, 0xcb, 0x32, 0xed, 0x11, 0xfc, 0xb2, 0xf7, 0x4b, 0x86, 0xf8, 0x7e, 0xfe,
]);

fn semantic_timeline_id(trace_id: &[u8], span_id: &[u8]) -> TimelineId {
    let mut bytes = trace_id.to_vec();
    bytes.extend_from_slice(span_id);
    TimelineId::from(Uuid::new_v5(&NAMESPACE_OTEL, &bytes))
}

fn fork_nonce(trace_id: &[u8], parent_span_id: &[u8], child_span_id: &[u8]) -> i64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(trace_id);
    hasher.write(parent_span_id);
    hasher.write(child_span_id);
    hasher.write(b"fork");
    let hash: u64 = hasher.finish();
    unsafe { std::mem::transmute(hash) }
}

fn join_nonce(trace_id: &[u8], parent_span_id: &[u8], child_span_id: &[u8]) -> i64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(trace_id);
    hasher.write(parent_span_id);
    hasher.write(child_span_id);
    hasher.write(b"join");
    let hash: u64 = hasher.finish();
    unsafe { std::mem::transmute(hash) }
}

fn prefixed_key(mut prefix: Vec<String>, key: String) -> String {
    if !prefix.is_empty() {
        prefix.push(key);
        prefix.join(".")
    } else {
        key
    }
}

fn otlp_kvs_to_modality(prefix: Vec<String>, kvs: Vec<KeyValue>, attrs: &mut AttrMap) {
    for kv in kvs.into_iter() {
        if let Some(otlp_val) = kv.value {
            otlp_kv_to_modality(prefix.clone(), kv.key, otlp_val, attrs)
        }
    }
}

fn otlp_kv_to_modality(prefix: Vec<String>, key: String, otlp_val: AnyValue, attrs: &mut AttrMap) {
    if let Some(otlp_val) = otlp_val.value {
        match otlp_val {
            any_value::Value::StringValue(s) => {
                attrs.insert(prefixed_key(prefix, key), s);
            }
            any_value::Value::BoolValue(b) => {
                attrs.insert(prefixed_key(prefix, key), b);
            }
            any_value::Value::IntValue(i) => {
                attrs.insert(prefixed_key(prefix, key), i);
            }
            any_value::Value::DoubleValue(d) => {
                attrs.insert(prefixed_key(prefix, key), d);
            }
            any_value::Value::ArrayValue(a) => {
                let mut p = prefix;
                p.push(key);
                for (i, val) in a.values.into_iter().enumerate() {
                    otlp_kv_to_modality(p.clone(), i.to_string(), val, attrs);
                }
            }
            any_value::Value::KvlistValue(kvlist) => {
                let mut p = prefix;
                p.push(key);
                for kv in kvlist.values.into_iter() {
                    if let Some(val) = kv.value {
                        otlp_kv_to_modality(p.clone(), kv.key, val, attrs)
                    }
                }
            }
            any_value::Value::BytesValue(bytes) => {
                attrs.insert(prefixed_key(prefix, key), hex::encode(&bytes));
            }
        }
    }
}
