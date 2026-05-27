use crate::{config::SmtpConfig, message::Message};
use lettre::{
    message::{header::ContentType, Mailbox, MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message as Email, Tokio1Executor,
};

/// Fire-and-forget: send an email notification for a published message.
/// Errors are logged and swallowed — email failure must never block publish.
pub async fn send_notification(smtp: &SmtpConfig, msg: &Message) {
    // Skip if message priority is below the configured threshold.
    if smtp.min_priority > 0 && msg.priority < smtp.min_priority as i32 {
        return;
    }

    let subject = build_subject(msg);
    let body = build_body(msg);

    let from: Mailbox = match smtp.from.parse() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "email: invalid smtp_from address");
            return;
        }
    };

    let transport = match build_transport(smtp) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "email: failed to build SMTP transport");
            return;
        }
    };

    for recipient in &smtp.to {
        let to: Mailbox = match recipient.parse() {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(recipient, error = %e, "email: invalid recipient address, skipping");
                continue;
            }
        };

        let email = match Email::builder()
            .from(from.clone())
            .to(to)
            .subject(&subject)
            .multipart(
                MultiPart::alternative()
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_PLAIN)
                            .body(body.clone()),
                    ),
            ) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(recipient, error = %e, "email: failed to build message");
                continue;
            }
        };

        match transport.send(email).await {
            Ok(_) => tracing::debug!(recipient, topic = %msg.topic, "email: notification sent"),
            Err(e) => tracing::warn!(recipient, topic = %msg.topic, error = %e, "email: send failed"),
        }
    }
}

fn build_subject(msg: &Message) -> String {
    if msg.title.is_empty() {
        format!("[ntfy/{}]", msg.topic)
    } else {
        format!("[ntfy/{}] {}", msg.topic, msg.title)
    }
}

fn build_body(msg: &Message) -> String {
    let mut parts: Vec<String> = Vec::new();

    if !msg.message.is_empty() {
        parts.push(msg.message.clone());
    }

    if !msg.tags.is_empty() {
        parts.push(format!("Tags: {}", msg.tags.join(", ")));
    }

    if !msg.click.is_empty() {
        parts.push(format!("Link: {}", msg.click));
    }

    if let Some(ref att) = msg.attachment {
        parts.push(format!("Attachment: {} ({})", att.name, att.url));
    }

    parts.push(format!("Topic: {}", msg.topic));
    parts.push(format!("Priority: {}", msg.priority));

    parts.join("\n")
}

fn build_transport(
    smtp: &SmtpConfig,
) -> anyhow::Result<AsyncSmtpTransport<Tokio1Executor>> {
    let creds = Credentials::new(smtp.username.clone(), smtp.password.clone());

    let transport = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)?
        .port(smtp.port)
        .credentials(creds)
        .build();

    Ok(transport)
}
