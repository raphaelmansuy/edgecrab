---
title: SMS Setup (Twilio)
description: Connect EdgeCrab to SMS via Twilio REST API and webhook. Grounded in crates/edgecrab-gateway/src/sms.rs.
sidebar:
  order: 9
---

The SMS adapter sends outbound messages via the Twilio REST API and receives inbound messages via an axum webhook endpoint.

**Max message length**: 1600 characters (~10 SMS segments). Longer responses are automatically truncated.

---

## Prerequisites

1. A [Twilio account](https://twilio.com) with SMS capability
2. A Twilio phone number (SMS-enabled, E.164 format)
3. A publicly reachable webhook URL (or use `ngrok` for local development)

---

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `TWILIO_ACCOUNT_SID` | **Yes** | Twilio Account SID (from console) |
| `TWILIO_AUTH_TOKEN` | **Yes** | Twilio Auth Token (from console) |
| `TWILIO_PHONE_NUMBER` | **Yes** | Your Twilio number in E.164 format (e.g. `+15551234567`) |
| `SMS_WEBHOOK_PORT` | No | Local webhook port (default: `8082`) |
| `SMS_ALLOWED_USERS` | No | Comma-separated allowed phone numbers (E.164) |

---

## Setup

### 1. Get Twilio Credentials

Go to [console.twilio.com](https://console.twilio.com) and copy:
- **Account SID** and **Auth Token** from the Dashboard
- Your Twilio **Phone Number** (buy one if needed)

### 2. Set Environment Variables

```bash
# ~/.edgecrab/.env
TWILIO_ACCOUNT_SID=ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
TWILIO_AUTH_TOKEN=your_auth_token
TWILIO_PHONE_NUMBER=+15551234567
SMS_ALLOWED_USERS=+14155551234,+16505559876
```

### 3. Configure Twilio Webhook

In the Twilio console, set your phone number's **SMS webhook** to:

```
https://your-public-ip:8082/sms
```

For local development, use ngrok:

```bash
ngrok http 8082
# Copy the https URL, e.g. https://abc123.ngrok.io
# Set Twilio webhook to: https://abc123.ngrok.io/sms
```

### 4. Start the Gateway

```bash
edgecrab gateway start
```

---

## Usage

Send an SMS to your Twilio number. EdgeCrab responds to the same number. Each phone number maintains its own persistent conversation session.

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| No reply received | Webhook not reachable | Check ngrok/reverse proxy and Twilio webhook URL |
| `21606` error | Phone number not SMS-capable | Verify number in Twilio console |
| Replies truncated | > 1600 chars | Normal — longer responses auto-chunk into sequential SMS messages |

---

## Pro Tips

- **SMS is one-way-oriented:** No slash commands (no `/help`, `/new`, etc.) — SMS has no distinction between a command message and a regular message. Start fresh by simply texting "start over" or "new conversation".
- **`SMS_ALLOWED_USERS` is critical for cost control.** An open webhook means anyone who discovers your Twilio number can run your agent and rack up Twilio charges. Always restrict to known numbers in production.
- **Use `ngrok` for local testing only.** ngrok tunnels break on restart (different URL each time). For production, use a stable reverse proxy (Caddy, nginx) with a fixed public IP or domain.
- **Twilio message segments:** Each 160-character block is one SMS segment. A 1,600-char response = ~10 segments. At Twilio's pricing (~$0.0079/segment in the US), a verbose agent response costs cents rather than fractions of a cent.
- **Test your webhook URL directly:**  
  ```bash
  curl -X POST http://localhost:8082/sms \
    -d 'From=%2B14155551234&Body=hello'
  ```
  If EdgeCrab responds, the webhook is working — the issue is Twilio's ability to reach your server.

---

## FAQ

**Q: Can I use other SMS providers besides Twilio?**  
Not currently. The SMS adapter is built specifically for the Twilio REST API v2010. Other providers (Vonage, Amazon SNS) would need custom adapter implementations.

**Q: How does EdgeCrab identify users via SMS?**  
By the `From` phone number in each Twilio webhook request. Each unique phone number gets its own persistent session.

**Q: Does EdgeCrab support MMS (images via SMS)?**  
Not currently. Twilio delivers MMS as a URL in the webhook. EdgeCrab sees the text portion of the message only.

**Q: My Twilio webhook returns 200 but EdgeCrab doesn't reply.**  
Check `SMS_ALLOWED_USERS` — if set, the sending number must be in the list (E.164 format: `+12125551234`). Check `RUST_LOG=debug` output for the gateway session processing.

**Q: Can I run the SMS webhook on HTTPS?**  
Yes — recommended for production. Twilio validates HTTPS webhooks. Use a reverse proxy (Caddy, nginx with certbot) to terminate TLS in front of EdgeCrab's HTTP listener.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) — Multi-platform setup
- [Security Model](/user-guide/security/) — Access control
- [Self-Hosting Guide](/guides/self-hosting/) — Production deployment with TLS
