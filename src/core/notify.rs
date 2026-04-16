use crate::core::config::{SmtpConfig, SmtpTls};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

/// Sends a backup result notification via SMTP.
///
/// Called after every backup run when `[notify]` is configured.
/// On success, a notification is only sent when `config.on_success = true`.
/// On failure, a notification is always sent.
///
/// Errors from the SMTP send are printed to stderr but never propagate — a
/// notification failure must not mask the original backup error.
pub fn send_backup_result(cfg: &SmtpConfig, succeeded: bool, src: &str, detail: &str) {
    if succeeded && !cfg.on_success {
        return;
    }

    let subject = if succeeded {
        format!("Backup OK: {src}")
    } else {
        format!("Backup FAILED: {src}")
    };

    let body = if succeeded {
        format!("Backup of '{src}' completed successfully.\n")
    } else {
        format!("Backup of '{src}' failed with the following error:\n\n{detail}\n")
    };

    if let Err(e) = send(cfg, &subject, &body) {
        eprintln!("warning: failed to send notification email: {e}");
    }
}

fn send(cfg: &SmtpConfig, subject: &str, body: &str) -> Result<(), lettre::transport::smtp::Error> {
    let email = Message::builder()
        .from(cfg.from.parse().expect("invalid notify.from address"))
        .to(cfg.to.parse().expect("invalid notify.to address"))
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .expect("failed to build email message");

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
