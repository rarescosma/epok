use cmd_lib::run_fun;
use lazy_static::lazy_static;
use tokio::time::Duration;

pub mod batch;
pub mod cli;
pub mod executor;
pub mod iptables;
pub mod logging;
pub mod operator;
pub mod res;
pub mod state;

lazy_static! {
    static ref ARG_MAX: String = {
        let default = 8192_u32;
        let res = run_fun!(getconf ARG_MAX).unwrap_or_else(|_| default.to_string());
        format!(
            "{:.0}",
            (res.parse::<u32>().unwrap_or(default) as f32 * 0.8)
        )
    };
}

pub use batch::*;
pub use cli::*;
pub use executor::*;
pub use iptables::*;
pub use logging::*;
pub use operator::*;
pub use res::{ExternalPort, Node, Service};
pub use state::{Interface, Op, Ops, State};

pub use k8s_openapi::api::core::v1::{Node as CoreNode, Service as CoreService};

pub const ANNOTATION: &str = "epok.getbetter.ro/externalport";
pub const NODE_EXCLUDE_ANNOTATION: &str = "epok.getbetter.ro/exclude";
pub const NODE_EXCLUDE_LABEL: &str = "epok_exclude";
pub const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(100);
pub const MAX_OP_QUEUE_SIZE: usize = 64;
pub const RULE_MARKER: &str = "epok_rule_id";
pub const SERVICE_MARKER: &str = "epok_service_id";
