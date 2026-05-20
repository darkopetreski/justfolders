use crate::core::config::{SmtpConfig, SmtpTls};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

/// Outcome of a backup run as far as the notification layer is concerned.
///
/// Maps onto three distinct email subjects: `Backup OK`, `Backup WARNING`,
/// and `Backup FAILED`.
pub enum NotifyState<'a> {
    /// Clean run — every file transferred.
    Success,
    /// Partial run — some files were skipped but the snapshot was created.
    /// `stderr` is the captured rsync stderr so the email can list which
    /// files were skipped and why.
    Warnings { stderr: &'a str },
    /// Hard failure — the snapshot was not created.
    Failed { detail: &'a str },
}

/// Sends a backup result notification via SMTP.
///
/// Called after every backup run when `[notify]` is configured.  The
/// `on_success` and `on_warnings` flags in [`SmtpConfig`] decide whether
/// the `Success` and `Warnings` states are actually delivered; the
/// `Failed` state is always delivered.
///
/// Errors from the SMTP send are printed to stderr but never propagate — a
/// notification failure must not mask the original backup error.
pub fn send_backup_result(cfg: &SmtpConfig, state: NotifyState, src: &str) {
    let (subject, body) = match state {
        NotifyState::Success => {
            if !cfg.on_success {
                return;
            }
            (
                format!("Backup OK: {src}"),
                format!("Backup of '{src}' completed successfully.\n"),
            )
        }
        NotifyState::Warnings { stderr } => {
            if !cfg.on_warnings {
                return;
            }
            (
                format!("Backup WARNING: {src}"),
                format!(
                    "Backup of '{src}' completed with warnings — some files were not transferred \
                     (typically permission denied on individual files, or files that vanished \
                     mid-transfer). The snapshot was still created from the files that were \
                     transferred.\n\n\
                     Rsync output:\n\n{stderr}\n"
                ),
            )
        }
        NotifyState::Failed { detail } => (
            format!("Backup FAILED: {src}"),
            format!("Backup of '{src}' failed with the following error:\n\n{detail}\n"),
        ),
    };

    if let Err(e) = send(cfg, &subject, &body) {
        eprintln!("warning: failed to send notification email: {e}");
    }
}

/// Sends a test email using the current SMTP settings.
///
/// Used by the `test-notify` CLI subcommand to check that the `[notify]`
/// section of the config is wired up correctly, without waiting for a real
/// backup failure to find out.  Unlike [`send_backup_result`], errors are
/// returned to the caller so the CLI can show them to the user and exit with
/// a non-zero status.
pub fn send_test_email(cfg: &SmtpConfig) -> Result<(), lettre::transport::smtp::Error> {
    let subject = "JustFolders: SMTP test email";
    let body = "This is a test email from JustFolders.\n\n\
                If you received this message, your [notify] SMTP settings \
                are working correctly and you will receive backup notifications.\n";
    send(cfg, subject, body)
}

/// Sends a single email via SMTP using the settings in `cfg`.
///
/// The `from` and `to` addresses were validated as parseable `Mailbox` values
/// when the config was loaded, so the `.parse().unwrap()` calls here are safe —
/// a bad address would have produced a `ConfigError` at startup.
fn send(cfg: &SmtpConfig, subject: &str, body: &str) -> Result<(), lettre::transport::smtp::Error> {
    // Both addresses were validated as parseable Mailboxes in SmtpConfig::load,
    // so these unwraps are safe — a bad address is caught at config load time.
    let email = Message::builder()
        .from(cfg.from.parse().unwrap())
        .to(cfg.to.parse().unwrap())
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .unwrap();

    let creds = Credentials::new(cfg.smtp_user.clone(), cfg.smtp_password.clone());

    let transport = match cfg.tls {
        SmtpTls::StartTls => SmtpTransport::starttls_relay(&cfg.smtp_host)?
            .port(cfg.smtp_port)
            .credentials(creds)
            .build(),
        SmtpTls::Tls => SmtpTransport::relay(&cfg.smtp_host)?
            .port(cfg.smtp_port)
            .credentials(creds)
            .build(),
        SmtpTls::None => SmtpTransport::builder_dangerous(&cfg.smtp_host)
            .port(cfg.smtp_port)
            .credentials(creds)
            .build(),
    };

    transport.send(&email)?;
    Ok(())
}
