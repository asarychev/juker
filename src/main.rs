use anyhow::Result;
use clap::Parser;
use juker::{
    ConnectionInfo, JuHelpLink, JuKernel, JuKernelInfo,
    message::{EvalResult, EvalValue},
    server::JuServer,
};
use serde_json::json;
use std::{env, fs::File, path::PathBuf};
use tracing::{debug, error, info, level_filters::LevelFilter, trace, warn};
use tracing_subscriber::EnvFilter;
use tracing_udp::UdpTracingWriter;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct JupyterApplication {
    /// Sets a custom config file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
    #[arg(short = 'C', long)]
    connection_file: PathBuf,
    /// Turn debugging information on
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
    // #[command(subcommand)]
    // command: JupyterCommands,
}

// #[derive(Subcommand)]
// enum JupyterCommands {
//     Open(Box<OpenAction>),
//     Start(Box<StartAction>),
//     Install(Box<InstallAction>),
//     Uninstall(Box<UninstallAction>),
// }

impl JupyterApplication {
    pub async fn run(&self) -> Result<()> {
        error!("Error log example");
        warn!("Warning log example");
        info!("Info log example");
        debug!("Debug log example");
        trace!("Trace log example");

        let f = File::open(&self.connection_file)?;
        info!("Opened connection file: {:?}", f);

        let ci: ConnectionInfo = serde_json::from_reader(f)?;
        info!("Connection file content: {:?}", ci);

        loop {
            let eva = Eva {};
            let res = JuServer::start(&ci, eva).await;

            match &res {
                Ok(true) => {
                    info!("Server exited and requested restart, restarting.");
                    continue;
                }
                Ok(false) => {
                    info!("Server exited successfully.");
                }
                Err(e) => {
                    error!("Server error: {:?}", e);
                }
            }
            break;
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::DEBUG.into())
                .from_env_lossy(),
        )
        .with_writer(UdpTracingWriter::new("localhost:5555")?)
        // Use a more compact, abbreviated log format
        .compact()
        // Display source code file paths
        .with_file(true)
        // Display source code line numbers
        .with_line_number(true)
        // // Display the thread ID an event was recorded on
        // .with_thread_ids(true)
        // // Don't display the event's target (module path)
        // .with_target(false)
        // // Build the subscriber
        .finish();
    // use that subscriber to process traces emitted after this point
    tracing::subscriber::set_global_default(subscriber)?;
    // let (non_blocking, _guard) = tracing_appender::non_blocking(std::io::stdout());

    // tracing_subscriber
    //     ::registry()
    //     .with(fmt::layer().with_writer(non_blocking))
    //     .with(
    //         EnvFilter::builder().with_default_directive(LevelFilter::DEBUG.into()).from_env_lossy()
    //     )
    //     .init();

    let args: Vec<String> = env::args().collect();

    debug!("args: {args:?}");

    let app = JupyterApplication::parse();
    let res = app.run().await;
    match &res {
        Ok(_) => {
            error!("Application exited successfully");
        }
        Err(e) => {
            error!("Application error: {:?}, bt:\n{:?}", e, e.backtrace());
        }
    };

    res
}

struct Eva;

impl JuKernel for Eva {
    fn kernel_info(&self) -> JuKernelInfo {
        JuKernelInfo {
            name: "testing".to_string(),
            version: "0.0.0".to_string(),
            mimetype: "text/testing".to_string(),
            file_extension: ".testing".to_string(),
            banner: "Juker Test Jupyter Kernel".to_string(),
            help_links: vec![JuHelpLink {
                text: "Juker Documentation".to_string(),
                url: "https://github.com/asaryche/juker".to_string(),
            }],
        }
    }

    async fn eval_code(&mut self, code: String) -> EvalResult {
        if code.starts_with("err") {
            EvalResult::Error {
                ename: json!("Error"),
                evalue: json!("An error occurred during code execution"),
                traceback: vec![json!("Traceback (most recent call last):"), json!("  ...")],
            }
        } else {
            EvalResult::Success {
                results: vec![EvalValue {
                    data: json!({
                        "text/plain": format!("Executed code: {}", code),
                    }),
                    metadata: json!({}),
                }],
            }
        }
    }
}
