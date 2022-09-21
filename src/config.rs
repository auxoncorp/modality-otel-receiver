use crate::auth::{AuthTokenBytes, AuthTokenError};
use crate::opts::Opts;
use crate::DynError;
use modality_reflector_config::{Config, TomlValue, TopLevelIngest, CONFIG_ENV_VAR};
use serde::Deserialize;
use std::env;
use std::net::SocketAddr;
use std::path::Path;
use url::Url;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct OtelReceiverConfig {
    pub auth_token: Option<String>,
    pub ingest: TopLevelIngest,
    pub plugin: PluginConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
pub struct PluginConfig {
    pub run_id: Option<Uuid>,
    pub otlp_addr: Option<SocketAddr>,
}

impl OtelReceiverConfig {
    pub fn load_merge_with_opts(opts: Opts) -> Result<Self, DynError> {
        let cfg = if let Some(cfg_path) = &opts.config_file {
            modality_reflector_config::try_from_file(cfg_path)?
        } else if let Ok(env_path) = env::var(CONFIG_ENV_VAR) {
            modality_reflector_config::try_from_file(Path::new(&env_path))?
        } else {
            Config::default()
        };

        let mut ingest = cfg.ingest.clone().unwrap_or_default();
        if let Some(url) = &opts.protocol_parent_url {
            ingest.protocol_parent_url = Some(url.clone());
        }
        if opts.allow_insecure_tls {
            ingest.allow_insecure_tls = true;
        }

        let mut plugin = PluginConfig::from_cfg_metadata(&cfg)?;
        if opts.run_id.is_some() {
            plugin.run_id = opts.run_id;
        }
        if opts.otlp_addr.is_some() {
            plugin.otlp_addr = opts.otlp_addr;
        }

        Ok(Self {
            auth_token: opts.auth_token,
            ingest,
            plugin,
        })
    }

    pub fn protocol_parent_url(&self) -> Result<Url, url::ParseError> {
        if let Some(url) = &self.ingest.protocol_parent_url {
            Ok(url.clone())
        } else {
            let url = Url::parse("modality-ingest://127.0.0.1:14188")?;
            Ok(url)
        }
    }

    pub fn resolve_auth(&self) -> Result<AuthTokenBytes, AuthTokenError> {
        AuthTokenBytes::resolve(self.auth_token.as_deref())
    }

    pub fn otlp_addr(&self) -> SocketAddr {
        self.plugin
            .otlp_addr
            .unwrap_or_else(|| "127.0.0.1:4317".parse().unwrap())
    }
}

impl PluginConfig {
    fn from_cfg_metadata(cfg: &Config) -> Result<PluginConfig, DynError> {
        let cfg = TomlValue::Table(cfg.metadata.clone().into_iter().collect()).try_into()?;
        Ok(cfg)
    }
}
