---
title: Email Setup
description: Connect EdgeCrab to email via SendGrid, Mailgun, or generic SMTP relay with an inbound webhook. Grounded in crates/edgecrab-gateway/src/email.rs.
sidebar:
  order: 10
---

The Email adapter sends outbound email via HTTP relay APIs (SendGrid, Mailgun, or generic SMTP relay) and receives inbound email via an axum webhook endpoint.

**Max message length**: 50,000 characters.

---

## Supported Providers

| Provider | `EMAIL_PROVIDER` value | Notes |
|----------|------------------------|-------|
| SendGrid | `sendgrid` | Requires inbound parse webhook setup |
| Mailgun | `mailgun` | Requires `EMAIL_DOMAIN` |
| Generic SMTP relay | `generic_smtp` | For self-hosted or other providers |

---

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `EMAIL_PROVIDER` | **Yes** | One of: `sendgrid`, `mailgun`, `generic_smtp` |
| `EMAIL_API_KEY` | Conditional | Required for `sendgrid` and `mailgun`. Optional fallback password for `generic_smtp`. |
| `EMAIL_FROM` | **Yes** | Sender address (e.g. `bot@example.com`) |
| `EMAIL_DOMAIN` | Conditional | Required for `mailgun` (e.g. `mg.example.com`) |
| `EMAIL_SMTP_HOST` | Conditional | Required for `generic_smtp` |
| `EMAIL_SMTP_PORT` | No | SMTP port for `generic_smtp` (default: `587`) |
| `EMAIL_SMTP_USERNAME` | No | SMTP username for `generic_smtp` (defaults to `EMAIL_FROM`) |
| `EMAIL_SMTP_PASSWORD` | Conditional | SMTP password for `generic_smtp`. If omitted, `EMAIL_API_KEY` is used instead. |
| `EMAIL_WEBHOOK_PORT` | No | Inbound webhook port (default: `8093`) |
| `EMAIL_ALLOWED` | No | Comma-separated allowed sender addresses |

---

## Setup

### SendGrid

1. Create a [SendGrid account](https://sendgrid.com) and verify your sender domain
2. Create an API key with **Mail Send** permission
3. In SendGrid **Settings → Inbound Parse**, configure:
   - **Hostname**: your mail domain (e.g. `bot.example.com`)
   - **URL**: `https://your-server:8093/email`

```bash
# ~/.edgecrab/.env
EMAIL_PROVIDER=sendgrid
EMAIL_API_KEY=SG.xxxxxxxxxxxxx
EMAIL_FROM=edgecrab@example.com
EMAIL_ALLOWED=you@example.com,team@example.com
```

### Mailgun

1. Add and verify a domain in [Mailgun](https://mailgun.com)
2. Get your Sending API key
3. Set up **Routes** → **Forward** to `https://your-server:8093/email`

```bash
EMAIL_PROVIDER=mailgun
EMAIL_API_KEY=key-xxxxxxxxxxxxx
EMAIL_FROM=edgecrab@mg.example.com
EMAIL_DOMAIN=mg.example.com
```

### Generic SMTP

```bash
EMAIL_PROVIDER=generic_smtp
EMAIL_FROM=edgecrab@example.com
EMAIL_SMTP_HOST=smtp.example.com
EMAIL_SMTP_PORT=587
EMAIL_SMTP_USERNAME=edgecrab@example.com
EMAIL_SMTP_PASSWORD=super-secret-password
```

### Start the Gateway

```bash
edgecrab gateway start
```

---

## Usage

Send an email to the `EMAIL_FROM` address. EdgeCrab replies to the `Reply-To` address in the email. Each unique sender address maintains its own session.

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| No response | Inbound parse not configured | Follow provider-specific webhook setup |
| `AUTH_ERROR` | Wrong provider credentials | Verify `EMAIL_API_KEY` for SendGrid/Mailgun or `EMAIL_SMTP_*` for SMTP |
| Too many sessions | Each sender gets own session | Use `EMAIL_ALLOWED` to restrict senders |

---

## Pro Tips

- **Email is best for long-form, async tasks.** Unlike chat platforms, email recipients expect latency — use EdgeCrab's email adapter for batch analysis, report generation, or document review that takes minutes rather than seconds.
- **`Reply-To` vs. `From`:** EdgeCrab replies to the `Reply-To` header if present, falling back to `From`. Set `Reply-To` explicitly in your email client to route replies to a different address (e.g., a team inbox).
- **Subject line as context:** The email subject is passed to EdgeCrab as part of the message context. Use descriptive subjects like `Review PR #123` instead of `Hey` — it helps the agent understand task scope immediately.
- **Session isolation per sender:** Each unique `From` address gets its own persistent session. Use `EMAIL_ALLOWED` to restrict to known senders, which also limits session proliferation.
- **Local testing:** Use [`mailhog`](https://github.com/mailhog/MailHog) or [`inbucket`](https://www.inbucket.org/) to capture emails locally, then forward them manually to the webhook:
  ```bash
  curl -X POST http://localhost:8093/email \
    -F 'from=you@test.com' \
    -F 'subject=test' \
    -F 'text=hello world'
  ```

---

## FAQ

**Q: Does EdgeCrab support attachments in emails?**  
Text body only in the current version. Attachments are not parsed. Future versions may add PDF/document handling via the vision tool.

**Q: Can I use Gmail directly?**  
Gmail doesn't support custom inbound parse webhooks. Use SendGrid or Mailgun as a relay with forwarding from your Gmail address set up in the provider dashboard.

**Q: What happens if multiple emails arrive simultaneously?**  
Each email is queued per-sender. If a sender's session is already running an agent request, the new email is queued and processed after the current response finishes.

**Q: Does the email adapter support HTML emails?**  
The adapter extracts the plain-text part of the email (`text/plain`). HTML-only emails are delivered with HTML stripped to plain text.

**Q: Is there a rate limit on outbound emails?**  
EdgeCrab sends one email per agent response. The rate limit is your email provider's API limit (SendGrid: 100/s, Mailgun: varies by plan). EdgeCrab does not impose its own rate limit on email.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) — Multi-platform setup
- [Security Model](/user-guide/security/) — Access control and allowed senders
- [Self-Hosting Guide](/guides/self-hosting/) — Production deployment with TLS
- [Cron Jobs](/features/cron/) — Scheduled email reports via delivery targets
