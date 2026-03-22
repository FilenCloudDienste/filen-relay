mod api;
#[cfg(feature = "server")]
mod backend;
mod common;
mod components;
mod frontend;
mod util;

#[cfg(feature = "server")]
#[derive(clap::Parser, Clone)]
#[command(version)]
pub(crate) struct Args {
    #[arg(
        long,
        env = "FILEN_RELAY_ADMIN_EMAIL",
        help = "Email of the Filen account with admin privileges"
    )]
    admin_email: Option<String>,
    #[arg(
        long,
        env = "FILEN_RELAY_ADMIN_PASSWORD",
        help = "Password of the Filen account with admin privileges"
    )]
    admin_password: Option<String>,
    #[arg(
        long,
        env = "FILEN_RELAY_ADMIN_2FA_CODE",
        help = "2FA code of the Filen account with admin privileges (if 2FA is enabled)"
    )]
    admin_2fa_code: Option<String>,
    #[arg(
        long,
        env = "FILEN_RELAY_ADMIN_AUTH_CONFIG",
        help = "Auth config (export via Filen CLI) of the Filen account with admin privileges. You can use this instead of email/password/2fa for faster startup."
    )]
    admin_auth_config: Option<String>,
    #[arg(
        long,
        env = "FILEN_RELAY_DB_DIR",
        help = "Directory to store the database file. By default, the data will be stored in the admin's Filen drive."
    )]
    db_dir: Option<String>,
}

#[cfg(feature = "server")]
fn main() {
    if std::env::args()
        .any(|arg| arg.contains(backend::rclone_auth_proxy::ACT_AS_RCLONE_AUTH_PROXY_ARG))
    {
        // setup logging to file for easier debugging of rclone auth proxy process
        ftail::Ftail::new()
            .single_file(
                std::path::Path::new("filen-relay-rclone_auth_proxy.log"),
                true,
                log::LevelFilter::Debug,
            )
            .max_file_size(100)
            .init()
            .unwrap();

        if let Err(e) = backend::rclone_auth_proxy::rclone_auth_proxy_main() {
            log::error!("{:?}", e);
        }
    } else {
        backend::serve(<Args as clap::Parser>::parse());
    }
}

#[cfg(not(feature = "server"))]
fn main() {
    dioxus::launch(frontend::App);
}
