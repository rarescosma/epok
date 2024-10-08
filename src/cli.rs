use clap::{Args, Parser};

use super::ARG_MAX;

#[derive(Parser, Debug)]
#[clap(
    author,
    version,
    about,
    propagate_version = true,
    subcommand_value_name = "EXECUTOR",
    subcommand_help_heading = "EXECUTORS",
    disable_help_subcommand = true
)]
pub struct Opts {
    /// Comma-separated list of interfaces to forward packets from
    #[clap(
        long,
        short = 'i',
        value_parser,
        env = "EPOK_INTERFACES",
        display_order = 0
    )]
    pub interfaces: String,

    /// Internal services won't be reachable through this interface
    #[clap(long, env = "EPOK_EXTERNAL_INTERFACE")]
    pub external_interface: Option<String>,

    /// Internal services will be reachable through these IPs
    /// Format is: x.x.x.x/mask
    #[clap(long, env = "EPOK_EXTRA_INTERNAL_IPS")]
    pub extra_internal_ips: Option<String>,

    #[clap(flatten)]
    pub batch_opts: BatchOpts,

    #[clap(subcommand)]
    pub executor: Executor,
}

#[derive(Parser, Debug)]
pub struct BatchOpts {
    /// Batch the execution of iptables commands
    #[clap(long, env = "EPOK_BATCH_COMMANDS", default_value = "true")]
    pub batch_commands: bool,

    /// Maximum command batch size
    #[clap(long, env = "EPOK_BATCH_SIZE", default_value = &**ARG_MAX)]
    pub batch_size: usize,
}

#[derive(Parser, Debug)]
#[clap(long_about = "En taro Adun")]
pub enum Executor<Ssh: Args = SshHost> {
    /// Execute commands locally - use this executor when running epok directly
    /// on the host machine.
    Local,
    /// Execute commands through ssh - use this executor when running epok
    /// inside the Kubernetes cluster.
    Ssh(Ssh),
}

#[derive(Parser, Debug)]
pub struct SshHost {
    #[clap(short = 'H', long, value_parser, env = "EPOK_SSH_HOST")]
    pub host: String,
    #[clap(
        short = 'p',
        long,
        value_parser,
        env = "EPOK_SSH_PORT",
        default_value = "22"
    )]
    pub port: u16,
    #[clap(short = 'k', long = "key", value_parser, env = "EPOK_SSH_KEY")]
    pub key_path: String,
}
