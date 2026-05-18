use crate::core::config::{Config, ConfigError};
use crate::core::notify::send_test_email;
use crate::core::parser::TestNotifyArgs;
use thiserror::Error;

/// Errors that can occur when sending a test notification email.
#[derive(Debug, Error)]
pub enum TestNotifyError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    /// The config loaded fine but has no `[notify]` section, so there is
    /// nothing to test.
    #[error(
        "no [notify] section found in the config file — \
         add SMTP settings under [notify] before running test-notify"
    )]
    NotConfigured,

    /// The SMTP send itself failed (auth, connection, TLS, etc.).  The wrapped
    /// error from `lettre` contains the underlying reason and is shown to the
    /// user as part of the error message.
    #[error("failed to send test email: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
}

/// Sends a test email using the SMTP settings in the config file.
///
/// Unlike the notification path used after a backup run — which intentionally
/// swallows SMTP errors so they never mask the real backup result — this
/// command surfaces SMTP errors directly to the user and exits with a
/// non-zero status on failure.  That makes it safe to drop into cron or a
/// monitoring pipeline as a periodic check that notifications still work.
pub fn run(args: TestNotifyArgs) -> Result<(), TestNotifyError> {
    let config = Config::load(&args.config)?;
    let notify = config.notify.ok_or(TestNotifyError::NotConfigured)?;

    println!(
        "sending test email to {} via {}:{}...",
        notify.to, notify.smtp_host, notify.smtp_port
    );

    send_test_email(&notify)?;

    println!("test email sent successfully.");
    Ok(())
}
