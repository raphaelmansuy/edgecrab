---
title: Signal
description: Connect EdgeCrab to Signal via signal-cli HTTP daemon. Grounded in crates/edgecrab-gateway/src/signal.rs.
sidebar:
  order: 4
---

## Prerequisites

Signal requires a running [signal-cli](https://github.com/AsamK/signal-cli) HTTP daemon:

```bash
# Install signal-cli
brew install signal-cli             # macOS
apt install signal-cli              # Debian

# Register your number
signal-cli -u +1234567890 register
signal-cli -u +1234567890 verify 123456

# Start HTTP daemon
signal-cli -u +1234567890 daemon --http 127.0.0.1:8090
```

---

## Configuration

### Environment Variables (Quick Start)

```bash
export SIGNAL_HTTP_URL=http://127.0.0.1:8090
export SIGNAL_ACCOUNT=+1234567890
edgecrab gateway start
```

Both `SIGNAL_HTTP_URL` and `SIGNAL_ACCOUNT` must be set to auto-enable Signal.

### config.yaml

```yaml
gateway:
  signal:
    enabled: true
    http_url: ~          # signal-cli HTTP daemon URL (from SIGNAL_HTTP_URL)
    account: ~           # registered phone number (from SIGNAL_ACCOUNT)
    allowed_users: []    # phone numbers allowed to interact
```

---

## Usage

Send a message to the registered Signal number from any Signal client to start a session. Each sender gets their own agent session.

Signal provides end-to-end encryption — wire traffic between the Signal network and your device is encrypted. The plaintext is only visible to signal-cli and EdgeCrab on your server.

**Gateway slash commands** (send as regular Signal messages):

| Command | Effect |
|---------|--------|
| `/help` | List all available gateway commands |
| `/new` | Start a fresh conversation (clears history) |
| `/reset` | Alias for `/new` |
| `/stop` | Cancel the currently running agent response |
| `/retry` | Re-send your last message |
| `/status` | Show whether the agent is running or idle |
| `/usage` | Show session stats (running, queued, retryable) |
| `/hooks` | List loaded event hooks |

---

## Pro Tips

- **Keep signal-cli as a systemd service** so it stays running across reboots. EdgeCrab auto-reconnects if the HTTP daemon restarts, but you need signal-cli up first.
- **Use a dedicated phone number** (e.g., a VoIP number from Google Voice or MySudo) instead of your personal number. If the bot number is compromised, your personal number is unaffected.
- **Verify `signal-cli` is healthy** before starting EdgeCrab:
  ```bash
  curl http://127.0.0.1:8090/v1/health
  # Should return: {"status": "ok"}
  ```
- **Restrict `allowed_users`** using E.164-format phone numbers (e.g., `+14155551234`). Without this, anyone with your bot's number can interact with EdgeCrab.
- **Signal groups:** To use EdgeCrab in a Signal group, add the bot number as a group member. Messages from the group are dispatched to EdgeCrab under the group's ID as the session key.

---

## Troubleshooting

**signal-cli daemon won't start:**
```bash
# Check if already running
pgrep -a signal-cli
# Check logs
journalctl -u signal-cli -n 50
```
Common causes: another signal-cli process already bound to the port, missing `--http` flag in start command.

**"CaptchaRequired" during registration:**  
Signal sometimes requires CAPTCHA for new number registrations:
```bash
signal-cli -u +1234567890 register --captcha "your-captcha-token"
```
Get the token from https://signalcaptchas.org/registration/generate.html

**No response from EdgeCrab:**  
Verify signal-cli is receiving messages:
```bash
curl http://127.0.0.1:8090/v1/receive/+1234567890
```
This manually polls for pending messages. If results appear, signal-cli is working correctly.

**Messages received but EdgeCrab doesn't reply:**  
Check `SIGNAL_ACCOUNT` matches the registered number exactly (E.164 format: `+1234567890`).

---

## FAQ

**Q: Do I need a real SIM card to use Signal?**  
Signal requires a phone number for registration, but it can be a VoIP number (Google Voice, Twilio, MySudo). Some VoIP providers are blocked by Signal \u2014 Twilio Verify numbers work reliably.

**Q: Can EdgeCrab use Signal without a phone running?**  
Yes \u2014 signal-cli is a standalone Java program. You only need signal-cli running as a daemon on your server; no phone app is required after initial registration.

**Q: Is this as secure as a normal Signal conversation?**  
The Signal transport is E2E encrypted, but EdgeCrab runs on your server and sees plaintext messages. Security depends on your server security, not Signal's encryption.

**Q: How do I reset Signal registration if I lose the keys?**  
```bash
signal-cli -u +1234567890 unregister
signal-cli -u +1234567890 register
```
Warning: this invalidates all existing sessions.

**Q: signal-cli requires Java \u2014 is there a Docker option?**  
Yes: `docker pull bbernhard/signal-cli-rest-api` provides a pre-configured Docker image with the HTTP API already enabled.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) \u2014 Multi-platform setup
- [Security Model](/user-guide/security/) \u2014 Approval workflow and access control
- [Self-Hosting Guide](/guides/self-hosting/) \u2014 Running signal-cli + EdgeCrab together
- [Docker Guide](/user-guide/docker/) \u2014 Container-based deployment
