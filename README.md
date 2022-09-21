# modality-otel-receiver

A [Modality][modality] reflector plugin for receiving trace data in the OpenTelemetry OTLP wire protocol.

## Getting Started

1. Configure a modality reflector to run modality-otel-receiver (see
   Configuration below)
2. Configure your OpenTelemetry client library or relay
   (otel-collector) to point at the running receiver.

## Adapter Concept Mapping

The following describes the default mapping between OpenTelemetry's
concepts and [Modality's][modality] concepts. See the configuration
section for ways to change the default behavior.

* Each span is represented as a Modality timeline
  * The timeline begins with a start event, with the same name as the
    span. The event has the `event.span.id` and `event.span.name`
    attributes. `event.span.parent_span_id` is also set, when present.
  * All span events are then placed on the timeline. All event
    attributes are represented as modality attributes. Nested
    attributes are represented using a flattened key name.
  * The timeline ends with an end event, named `end_<spanname>`. 
  * `fork_<spanname>` and `join_<spanname>` events are generated in
    the parent span's timeline, positioned using the span start and
    end time (this is the extent of positioning information available
    in OTLP). Nonces are generated to track the `fork -> start` and
    `end -> join` interactions.
* Opentelemetry "resource" attributes are converted to timeline
  attributes for each timeline in the trace.
  * "service namespace" is stored as `timeline.service.namespace`
  * "service name" is stored as `timeline.service.name`
  * "service instance id" is stored as `timeline.service.instance.id`
* Open telementry 'scope' attributes are converted to timeline
  attributes, prefixed with `timeline.scope`.
* The timeline name is computed by joining the service namespace,
  service name, and span name with "." characters. (Segments are
  omitted if the data is not included)
* Timeline IDs are generated with a UUIDv5, based on a hash of the
  trace id and span id.
  * The trace id is also stored as the timeline attribute `timeline.trace.id`.


See the [Modality documentation](https://docs.auxon.io/modality/) for
more information on the Modality concepts.

## Configuration

All of the plugins can be configured through a TOML configuration file
(from either the `--config` option or the `MODALITY_REFLECTOR_CONFIG`
environment variable).  All of the configuration fields can optionally
be overridden at the CLI, see `--help` for more details.

See the [`modality-reflector` Configuration File
documentation](https://docs.auxon.io/modality/ingest/modality-reflector-configuration-file.html)
for more information about the reflector configuration.

* `[ingest]` — Top-level ingest configuration.
  - `additional-timeline-attributes` — Array of key-value attribute pairs to add to every timeline seen by the plugin.
  - `override-timeline-attributes` — Array of key-value attribute pairs to override on every timeline seen by this plugin.
  - `allow-insecure-tls` — Whether to allow insecure connections. Defaults to `true`.
  - `protocol-parent-url` — URL to which this reflector will send its collected data.

* `[plugins.ingest.collectors.otlp.metadata]` — Plugin configuration table. (just `metadata` if running standalone)
  - `run-id` — Use the provided UUID as the run ID instead of
    generating a random one.
  - `otlp-addr` — Listen for incoming OTLP gRPC connections at this
    address. If not given, listens for local connections only at the
    default port (127.0.0.1:4317)

## Building
- `git submodule init`, to get the protocol definitions
- `cargo build`

## LICENSE

See [LICENSE](./LICENSE) for more details.

Copyright 2022 [Auxon Corporation](https://auxon.io)

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

[http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0)

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

[modality]: https://auxon.io/products/modality

