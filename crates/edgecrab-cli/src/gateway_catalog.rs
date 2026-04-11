use edgecrab_core::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformState {
    Ready,
    Available,
    Incomplete,
    NotConfigured,
}

impl PlatformState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "enabled",
            Self::Available => "ready to enable",
            Self::Incomplete => "needs attention",
            Self::NotConfigured => "not configured",
        }
    }

    pub fn include_by_default(self) -> bool {
        !matches!(self, Self::NotConfigured)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupKind {
    Telegram,
    Discord,
    Slack,
    Signal,
    WhatsApp,
    GenericEnv,
    Webhook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Secret,
    Port,
}

#[derive(Debug, Clone, Copy)]
pub struct EnvField {
    pub key: &'static str,
    pub prompt: &'static str,
    pub help: &'static str,
    pub required: bool,
    pub kind: FieldKind,
    pub default_value: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct GatewayPlatformDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub setup_kind: SetupKind,
    pub instructions: &'static [&'static str],
    pub env_fields: &'static [EnvField],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformDiagnostic {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub setup_kind: SetupKind,
    pub state: PlatformState,
    pub detail: String,
    pub missing_required: Vec<&'static str>,
    pub active: bool,
}

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_test_env() -> std::sync::MutexGuard<'static, ()> {
    match TEST_ENV_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

const NO_FIELDS: &[EnvField] = &[];

const SMS_FIELDS: &[EnvField] = &[
    EnvField {
        key: "TWILIO_ACCOUNT_SID",
        prompt: "Twilio Account SID",
        help: "Find this in the Twilio Console dashboard.",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "TWILIO_AUTH_TOKEN",
        prompt: "Twilio Auth Token",
        help: "Use the Auth Token from your Twilio project.",
        required: true,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "TWILIO_PHONE_NUMBER",
        prompt: "Twilio phone number",
        help: "Use E.164 format such as +15551234567.",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "SMS_WEBHOOK_PORT",
        prompt: "SMS webhook port",
        help: "Inbound Twilio webhook port.",
        required: false,
        kind: FieldKind::Port,
        default_value: Some("8082"),
    },
    EnvField {
        key: "SMS_ALLOWED_USERS",
        prompt: "Allowed phone numbers",
        help: "Comma-separated E.164 numbers. Leave blank for open access.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
];

const MATRIX_FIELDS: &[EnvField] = &[
    EnvField {
        key: "MATRIX_HOMESERVER",
        prompt: "Matrix homeserver URL",
        help: "Example: https://matrix.org",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "MATRIX_ACCESS_TOKEN",
        prompt: "Matrix access token",
        help: "Use a long-lived access token for the bot account.",
        required: true,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "MATRIX_USER_ID",
        prompt: "Matrix bot user ID",
        help: "Optional full MXID such as @bot:matrix.org.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "MATRIX_ALLOWED_USERS",
        prompt: "Allowed Matrix user IDs",
        help: "Comma-separated MXIDs. Leave blank for open access.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
];

const MATTERMOST_FIELDS: &[EnvField] = &[
    EnvField {
        key: "MATTERMOST_URL",
        prompt: "Mattermost server URL",
        help: "Example: https://mattermost.example.com",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "MATTERMOST_TOKEN",
        prompt: "Mattermost bot token",
        help: "Create a bot or personal access token in Mattermost.",
        required: true,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "MATTERMOST_ALLOWED_USERS",
        prompt: "Allowed Mattermost user IDs",
        help: "Comma-separated user IDs. Leave blank for open access.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
];

const FEISHU_FIELDS: &[EnvField] = &[
    EnvField {
        key: "FEISHU_APP_ID",
        prompt: "Feishu App ID",
        help: "Create a custom app in Feishu or Lark and copy the App ID.",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "FEISHU_APP_SECRET",
        prompt: "Feishu App Secret",
        help: "Use the app secret from the bot application.",
        required: true,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "FEISHU_VERIFICATION_TOKEN",
        prompt: "Verification token",
        help: "Optional webhook verification token for callback validation.",
        required: false,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "FEISHU_ENCRYPT_KEY",
        prompt: "Encrypt key",
        help: "Optional Feishu webhook encrypt key for signature verification.",
        required: false,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "FEISHU_WEBHOOK_PORT",
        prompt: "Feishu webhook port",
        help: "Local HTTP port for Feishu event callbacks.",
        required: false,
        kind: FieldKind::Port,
        default_value: Some("8765"),
    },
    EnvField {
        key: "FEISHU_ALLOWED_USERS",
        prompt: "Allowed Feishu user IDs",
        help: "Comma-separated sender IDs. Leave blank for open access.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "FEISHU_GROUP_POLICY",
        prompt: "Feishu group policy",
        help: "Use mentioned, open, or disabled for group-chat ingress.",
        required: false,
        kind: FieldKind::Text,
        default_value: Some("mentioned"),
    },
    EnvField {
        key: "FEISHU_ALLOWED_GROUP_USERS",
        prompt: "Allowed group user IDs",
        help: "Optional comma-separated sender IDs allowed in group chats.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "FEISHU_BOT_OPEN_ID",
        prompt: "Feishu bot open_id",
        help: "Optional bot open_id used for precise @mention gating in groups.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "FEISHU_BOT_USER_ID",
        prompt: "Feishu bot user_id",
        help: "Optional bot user_id used for precise @mention gating in groups.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "FEISHU_BOT_NAME",
        prompt: "Feishu bot name",
        help: "Optional bot display name used as a final @mention fallback.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
];

const WECOM_FIELDS: &[EnvField] = &[
    EnvField {
        key: "WECOM_BOT_ID",
        prompt: "WeCom bot ID",
        help: "Use the bot ID assigned by the WeCom AI bot platform.",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "WECOM_SECRET",
        prompt: "WeCom bot secret",
        help: "Use the bot secret used during websocket subscription.",
        required: true,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "WECOM_WEBSOCKET_URL",
        prompt: "WeCom websocket URL",
        help: "Optional override for the WeCom AI Bot gateway websocket endpoint.",
        required: false,
        kind: FieldKind::Text,
        default_value: Some("wss://openws.work.weixin.qq.com"),
    },
    EnvField {
        key: "WECOM_ALLOWED_USERS",
        prompt: "Allowed WeCom user IDs",
        help: "Comma-separated sender IDs. Leave blank for open access.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
];

const DINGTALK_FIELDS: &[EnvField] = &[
    EnvField {
        key: "DINGTALK_APP_KEY",
        prompt: "DingTalk App Key",
        help: "This is the client identifier from the DingTalk Open Platform.",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "DINGTALK_APP_SECRET",
        prompt: "DingTalk App Secret",
        help: "Use the application secret from your DingTalk bot app.",
        required: true,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "DINGTALK_ROBOT_CODE",
        prompt: "DingTalk robot code",
        help: "Optional robot code used for message filtering.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
];

const HOME_ASSISTANT_FIELDS: &[EnvField] = &[
    EnvField {
        key: "HA_URL",
        prompt: "Home Assistant URL",
        help: "Example: http://homeassistant.local:8123",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "HA_TOKEN",
        prompt: "Home Assistant long-lived token",
        help: "Create this in your Home Assistant profile security page.",
        required: true,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "HA_ALLOWED_USERS",
        prompt: "Allowed Home Assistant user IDs",
        help: "Comma-separated user IDs. Leave blank for open access.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
];

const EMAIL_FIELDS: &[EnvField] = &[
    EnvField {
        key: "EMAIL_PROVIDER",
        prompt: "Email provider",
        help: "Use sendgrid, mailgun, or generic_smtp.",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "EMAIL_API_KEY",
        prompt: "Email provider API key",
        help: "Provider credential used for outbound email.",
        required: true,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "EMAIL_FROM",
        prompt: "From address",
        help: "Example: edgecrab@example.com",
        required: true,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "EMAIL_DOMAIN",
        prompt: "Mailgun domain",
        help: "Only needed when EMAIL_PROVIDER=mailgun.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "EMAIL_SMTP_HOST",
        prompt: "SMTP host",
        help: "Only needed when EMAIL_PROVIDER=generic_smtp.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "EMAIL_SMTP_PORT",
        prompt: "SMTP port",
        help: "Only needed when EMAIL_PROVIDER=generic_smtp. Defaults to 587.",
        required: false,
        kind: FieldKind::Port,
        default_value: Some("587"),
    },
    EnvField {
        key: "EMAIL_SMTP_USERNAME",
        prompt: "SMTP username",
        help: "Optional. Defaults to EMAIL_FROM when omitted.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
    EnvField {
        key: "EMAIL_SMTP_PASSWORD",
        prompt: "SMTP password",
        help: "Optional. Defaults to EMAIL_API_KEY when omitted.",
        required: false,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "EMAIL_WEBHOOK_PORT",
        prompt: "Email webhook port",
        help: "Inbound email webhook port.",
        required: false,
        kind: FieldKind::Port,
        default_value: Some("8093"),
    },
    EnvField {
        key: "EMAIL_ALLOWED",
        prompt: "Allowed sender emails",
        help: "Comma-separated email addresses. Leave blank for open access.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
];

const API_SERVER_FIELDS: &[EnvField] = &[
    EnvField {
        key: "API_SERVER_ENABLED",
        prompt: "Enable API server",
        help: "Set to true/1 to expose the OpenAI-compatible API.",
        required: true,
        kind: FieldKind::Text,
        default_value: Some("true"),
    },
    EnvField {
        key: "API_SERVER_HOST",
        prompt: "API server host",
        help: "Bind address for the OpenAI-compatible API server.",
        required: false,
        kind: FieldKind::Text,
        default_value: Some("127.0.0.1"),
    },
    EnvField {
        key: "API_SERVER_PORT",
        prompt: "API server port",
        help: "TCP port for the OpenAI-compatible API server.",
        required: false,
        kind: FieldKind::Port,
        default_value: Some("8642"),
    },
    EnvField {
        key: "API_SERVER_KEY",
        prompt: "API server bearer key",
        help: "Optional bearer token for API access.",
        required: false,
        kind: FieldKind::Secret,
        default_value: None,
    },
    EnvField {
        key: "API_SERVER_CORS_ORIGINS",
        prompt: "Allowed CORS origins",
        help: "Comma-separated origins. Leave blank for none.",
        required: false,
        kind: FieldKind::Text,
        default_value: None,
    },
];

const PLATFORMS: &[GatewayPlatformDef] = &[
    GatewayPlatformDef {
        id: "telegram",
        name: "Telegram",
        description: "Telegram Bot API",
        setup_kind: SetupKind::Telegram,
        instructions: &[
            "Create a bot with @BotFather.",
            "Copy the bot token and your Telegram user ID.",
            "Set a home channel if you want cron deliveries in Telegram.",
        ],
        env_fields: NO_FIELDS,
    },
    GatewayPlatformDef {
        id: "discord",
        name: "Discord",
        description: "Discord bot application",
        setup_kind: SetupKind::Discord,
        instructions: &[
            "Create a bot in the Discord developer portal.",
            "Enable the MESSAGE CONTENT intent.",
            "Invite the bot and copy the bot token.",
        ],
        env_fields: NO_FIELDS,
    },
    GatewayPlatformDef {
        id: "slack",
        name: "Slack",
        description: "Slack Socket Mode bot",
        setup_kind: SetupKind::Slack,
        instructions: &[
            "Create a Slack app and enable Socket Mode.",
            "Collect both the bot token and app token.",
            "Invite the bot to any channels you expect it to use.",
        ],
        env_fields: NO_FIELDS,
    },
    GatewayPlatformDef {
        id: "feishu",
        name: "Feishu",
        description: "Feishu or Lark bot webhook + REST adapter",
        setup_kind: SetupKind::GenericEnv,
        instructions: &[
            "Create a custom app in Feishu or Lark with bot and event permissions.",
            "Collect the App ID and App Secret.",
            "Point the platform event callback at your EdgeCrab Feishu webhook route.",
        ],
        env_fields: FEISHU_FIELDS,
    },
    GatewayPlatformDef {
        id: "wecom",
        name: "WeCom",
        description: "WeCom AI Bot websocket adapter",
        setup_kind: SetupKind::GenericEnv,
        instructions: &[
            "Create or open a WeCom AI bot integration.",
            "Collect the bot ID and secret used for websocket subscription.",
            "Allow outbound access to the WeCom websocket gateway from your host.",
        ],
        env_fields: WECOM_FIELDS,
    },
    GatewayPlatformDef {
        id: "signal",
        name: "Signal",
        description: "signal-cli HTTP daemon",
        setup_kind: SetupKind::Signal,
        instructions: &[
            "Run signal-cli in HTTP mode or docker-native mode.",
            "Configure the daemon URL and registered account number.",
            "Link the secondary device if the account is not registered yet.",
        ],
        env_fields: NO_FIELDS,
    },
    GatewayPlatformDef {
        id: "whatsapp",
        name: "WhatsApp",
        description: "Baileys bridge with QR pairing",
        setup_kind: SetupKind::WhatsApp,
        instructions: &[
            "Install Node.js 18+ and npm.",
            "Select bot mode or self-chat mode.",
            "Pair the bridge by scanning the QR code.",
        ],
        env_fields: NO_FIELDS,
    },
    GatewayPlatformDef {
        id: "webhook",
        name: "Webhook",
        description: "Always-on HTTP webhook adapter",
        setup_kind: SetupKind::Webhook,
        instructions: &[
            "Webhook uses the gateway bind address and does not need extra secrets by default.",
        ],
        env_fields: NO_FIELDS,
    },
    GatewayPlatformDef {
        id: "email",
        name: "Email",
        description: "HTTP email gateway",
        setup_kind: SetupKind::GenericEnv,
        instructions: &[
            "Choose a supported provider: sendgrid, mailgun, or generic_smtp.",
            "Configure the API key and sender address.",
            "Expose the webhook port if you need inbound email delivery.",
        ],
        env_fields: EMAIL_FIELDS,
    },
    GatewayPlatformDef {
        id: "sms",
        name: "SMS",
        description: "Twilio REST + webhook",
        setup_kind: SetupKind::GenericEnv,
        instructions: &[
            "Create a Twilio project with SMS enabled.",
            "Use a Twilio phone number in E.164 format.",
            "Point Twilio inbound webhooks at your EdgeCrab gateway.",
        ],
        env_fields: SMS_FIELDS,
    },
    GatewayPlatformDef {
        id: "matrix",
        name: "Matrix",
        description: "Matrix homeserver adapter",
        setup_kind: SetupKind::GenericEnv,
        instructions: &[
            "Use a Matrix bot account with an access token.",
            "Set the homeserver URL and optional bot MXID.",
        ],
        env_fields: MATRIX_FIELDS,
    },
    GatewayPlatformDef {
        id: "mattermost",
        name: "Mattermost",
        description: "Mattermost REST + WebSocket",
        setup_kind: SetupKind::GenericEnv,
        instructions: &[
            "Create a Mattermost bot or personal access token.",
            "Use the base server URL and token from the target workspace.",
        ],
        env_fields: MATTERMOST_FIELDS,
    },
    GatewayPlatformDef {
        id: "dingtalk",
        name: "DingTalk",
        description: "DingTalk Open Platform bot",
        setup_kind: SetupKind::GenericEnv,
        instructions: &[
            "Create an app in the DingTalk Open Platform.",
            "Collect the App Key and App Secret.",
        ],
        env_fields: DINGTALK_FIELDS,
    },
    GatewayPlatformDef {
        id: "homeassistant",
        name: "Home Assistant",
        description: "Home Assistant conversation adapter",
        setup_kind: SetupKind::GenericEnv,
        instructions: &[
            "Generate a long-lived Home Assistant access token.",
            "Use the base Home Assistant URL and optional user allowlist.",
        ],
        env_fields: HOME_ASSISTANT_FIELDS,
    },
    GatewayPlatformDef {
        id: "api_server",
        name: "API Server",
        description: "OpenAI-compatible HTTP server",
        setup_kind: SetupKind::GenericEnv,
        instructions: &[
            "Enable the embedded OpenAI-compatible API server if external tools need HTTP access.",
            "Set host, port, and optional bearer auth.",
        ],
        env_fields: API_SERVER_FIELDS,
    },
];

pub fn all_platforms() -> &'static [GatewayPlatformDef] {
    PLATFORMS
}

pub fn find_platform(id: &str) -> Option<&'static GatewayPlatformDef> {
    PLATFORMS
        .iter()
        .find(|platform| platform.id.eq_ignore_ascii_case(id))
}

pub fn collect_platform_diagnostics(config: &AppConfig) -> Vec<PlatformDiagnostic> {
    PLATFORMS
        .iter()
        .map(|platform| diagnose_platform(platform, config))
        .collect()
}

pub fn diagnose_platform(def: &GatewayPlatformDef, config: &AppConfig) -> PlatformDiagnostic {
    match def.id {
        "telegram" => single_token_platform(
            def,
            config
                .gateway
                .platform_requested(def.id, config.gateway.telegram.enabled),
            env_is_set("TELEGRAM_BOT_TOKEN"),
            "bot token present",
            "missing TELEGRAM_BOT_TOKEN",
        ),
        "discord" => single_token_platform(
            def,
            config
                .gateway
                .platform_requested(def.id, config.gateway.discord.enabled),
            env_is_set("DISCORD_BOT_TOKEN"),
            "bot token present",
            "missing DISCORD_BOT_TOKEN",
        ),
        "slack" => slack_platform(def, config),
        "signal" => signal_platform(def, config),
        "whatsapp" => whatsapp_platform(def, config),
        "webhook" => webhook_platform(def, config),
        "email" => email_platform(def, config),
        "api_server" => api_server_platform(def, config),
        _ => generic_env_platform(def, config),
    }
}

fn single_token_platform(
    def: &GatewayPlatformDef,
    enabled: bool,
    has_token: bool,
    ready_detail: &str,
    missing_detail: &str,
) -> PlatformDiagnostic {
    let (state, detail, missing_required, active) = if enabled && has_token {
        (
            PlatformState::Ready,
            ready_detail.to_string(),
            Vec::new(),
            true,
        )
    } else if !enabled && has_token {
        (
            PlatformState::Available,
            ready_detail.to_string(),
            Vec::new(),
            false,
        )
    } else if enabled {
        (
            PlatformState::Incomplete,
            missing_detail.to_string(),
            vec![if def.id == "telegram" {
                "TELEGRAM_BOT_TOKEN"
            } else {
                "DISCORD_BOT_TOKEN"
            }],
            false,
        )
    } else {
        (
            PlatformState::NotConfigured,
            "not configured yet".to_string(),
            Vec::new(),
            false,
        )
    };

    PlatformDiagnostic {
        id: def.id,
        name: def.name,
        description: def.description,
        setup_kind: def.setup_kind,
        state,
        detail,
        missing_required,
        active,
    }
}

fn slack_platform(def: &GatewayPlatformDef, config: &AppConfig) -> PlatformDiagnostic {
    let has_bot = env_is_set("SLACK_BOT_TOKEN");
    let has_app = env_is_set("SLACK_APP_TOKEN");
    let enabled = config
        .gateway
        .platform_requested(def.id, config.gateway.slack.enabled);

    let missing_required =
        required_missing(&[("SLACK_BOT_TOKEN", has_bot), ("SLACK_APP_TOKEN", has_app)]);
    let has_all = missing_required.is_empty();
    let detail = if has_all {
        "bot and app tokens present".to_string()
    } else if has_bot || has_app || enabled {
        format!("missing {}", missing_required.join(", "))
    } else {
        "not configured yet".to_string()
    };
    let state = if enabled && has_all {
        PlatformState::Ready
    } else if !enabled && has_all {
        PlatformState::Available
    } else if enabled || has_bot || has_app {
        PlatformState::Incomplete
    } else {
        PlatformState::NotConfigured
    };

    PlatformDiagnostic {
        id: def.id,
        name: def.name,
        description: def.description,
        setup_kind: def.setup_kind,
        state,
        detail,
        missing_required,
        active: enabled && has_all,
    }
}

fn signal_platform(def: &GatewayPlatformDef, config: &AppConfig) -> PlatformDiagnostic {
    let has_url = config
        .gateway
        .signal
        .http_url
        .as_deref()
        .is_some_and(not_blank)
        || env_is_set("SIGNAL_HTTP_URL");
    let has_account = config
        .gateway
        .signal
        .account
        .as_deref()
        .is_some_and(not_blank)
        || env_is_set("SIGNAL_ACCOUNT");
    let enabled = config
        .gateway
        .platform_requested(def.id, config.gateway.signal.enabled);
    let missing_required = required_missing(&[
        ("SIGNAL_HTTP_URL", has_url),
        ("SIGNAL_ACCOUNT", has_account),
    ]);
    let has_all = missing_required.is_empty();
    let detail = if has_all {
        "daemon URL and account configured".to_string()
    } else if enabled || has_url || has_account {
        format!("missing {}", missing_required.join(", "))
    } else {
        "not configured yet".to_string()
    };
    let state = if enabled && has_all {
        PlatformState::Ready
    } else if !enabled && has_all {
        PlatformState::Available
    } else if enabled || has_url || has_account {
        PlatformState::Incomplete
    } else {
        PlatformState::NotConfigured
    };

    PlatformDiagnostic {
        id: def.id,
        name: def.name,
        description: def.description,
        setup_kind: def.setup_kind,
        state,
        detail,
        missing_required,
        active: enabled && has_all,
    }
}

fn whatsapp_platform(def: &GatewayPlatformDef, config: &AppConfig) -> PlatformDiagnostic {
    let session_path = config
        .gateway
        .whatsapp
        .session_path
        .clone()
        .unwrap_or_else(edgecrab_gateway::whatsapp::WhatsAppAdapter::default_session_path);
    let paired = session_path.join("creds.json").exists();
    let enabled = config
        .gateway
        .platform_requested(def.id, config.gateway.whatsapp.enabled);
    let (state, detail, active) = if enabled && paired {
        (
            PlatformState::Ready,
            "paired session available".to_string(),
            true,
        )
    } else if !enabled && paired {
        (
            PlatformState::Available,
            "paired session available".to_string(),
            false,
        )
    } else if enabled {
        (
            PlatformState::Incomplete,
            "enabled but still needs QR pairing".to_string(),
            false,
        )
    } else {
        (
            PlatformState::NotConfigured,
            "not configured yet".to_string(),
            false,
        )
    };

    PlatformDiagnostic {
        id: def.id,
        name: def.name,
        description: def.description,
        setup_kind: def.setup_kind,
        state,
        detail,
        missing_required: Vec::new(),
        active,
    }
}

fn webhook_platform(def: &GatewayPlatformDef, config: &AppConfig) -> PlatformDiagnostic {
    let enabled = config.gateway.webhook_enabled;
    let state = if enabled {
        PlatformState::Ready
    } else {
        PlatformState::NotConfigured
    };
    let detail = if enabled {
        format!(
            "listening on {}:{}",
            config.gateway.host, config.gateway.port
        )
    } else {
        "disabled in config".to_string()
    };

    PlatformDiagnostic {
        id: def.id,
        name: def.name,
        description: def.description,
        setup_kind: def.setup_kind,
        state,
        detail,
        missing_required: Vec::new(),
        active: enabled,
    }
}

fn email_platform(def: &GatewayPlatformDef, config: &AppConfig) -> PlatformDiagnostic {
    let provider = std::env::var("EMAIL_PROVIDER").unwrap_or_default();
    let enabled = config.gateway.platform_enabled(def.id);
    let disabled = config.gateway.platform_disabled(def.id);
    let any_present = def.env_fields.iter().any(|field| env_is_set(field.key));
    let has_provider = not_blank(&provider);
    let provider_name = provider.trim().to_ascii_lowercase();

    let provider_supported = matches!(
        provider_name.as_str(),
        "sendgrid" | "mailgun" | "generic_smtp" | "smtp"
    );
    let missing_required = match provider_name.as_str() {
        "sendgrid" => required_missing(&[
            ("EMAIL_PROVIDER", has_provider),
            ("EMAIL_FROM", env_is_set("EMAIL_FROM")),
            ("EMAIL_API_KEY", env_is_set("EMAIL_API_KEY")),
        ]),
        "mailgun" => required_missing(&[
            ("EMAIL_PROVIDER", has_provider),
            ("EMAIL_FROM", env_is_set("EMAIL_FROM")),
            ("EMAIL_API_KEY", env_is_set("EMAIL_API_KEY")),
            ("EMAIL_DOMAIN", env_is_set("EMAIL_DOMAIN")),
        ]),
        "generic_smtp" | "smtp" => {
            let has_secret = env_is_set("EMAIL_SMTP_PASSWORD") || env_is_set("EMAIL_API_KEY");
            required_missing(&[
                ("EMAIL_PROVIDER", has_provider),
                ("EMAIL_FROM", env_is_set("EMAIL_FROM")),
                ("EMAIL_SMTP_HOST", env_is_set("EMAIL_SMTP_HOST")),
                ("EMAIL_SMTP_PASSWORD or EMAIL_API_KEY", has_secret),
            ])
        }
        _ if has_provider => required_missing(&[
            ("EMAIL_PROVIDER", false),
            ("EMAIL_FROM", env_is_set("EMAIL_FROM")),
        ]),
        _ => Vec::new(),
    };

    let has_all = has_provider && provider_supported && missing_required.is_empty();
    let state = if disabled && has_all {
        PlatformState::Available
    } else if has_all {
        PlatformState::Ready
    } else if enabled || any_present {
        PlatformState::Incomplete
    } else {
        PlatformState::NotConfigured
    };
    let detail = if !has_provider && (enabled || any_present) {
        "missing EMAIL_PROVIDER".to_string()
    } else if disabled && has_all {
        "credentials present but disabled".to_string()
    } else if has_all {
        match provider_name.as_str() {
            "sendgrid" => "SendGrid email delivery configured".to_string(),
            "mailgun" => "Mailgun email delivery configured".to_string(),
            "generic_smtp" | "smtp" => "SMTP email delivery configured".to_string(),
            _ => "email delivery configured".to_string(),
        }
    } else if provider_name == "mailgun" && missing_required == vec!["EMAIL_DOMAIN"] {
        "missing EMAIL_DOMAIN for mailgun".to_string()
    } else if matches!(provider_name.as_str(), "generic_smtp" | "smtp")
        && missing_required == vec!["EMAIL_SMTP_PASSWORD or EMAIL_API_KEY"]
    {
        "missing EMAIL_SMTP_PASSWORD or EMAIL_API_KEY for generic_smtp".to_string()
    } else if !missing_required.is_empty() {
        format!("missing {}", missing_required.join(", "))
    } else if has_provider && !provider_supported {
        "unsupported EMAIL_PROVIDER".to_string()
    } else {
        "not configured yet".to_string()
    };

    PlatformDiagnostic {
        id: def.id,
        name: def.name,
        description: def.description,
        setup_kind: def.setup_kind,
        state,
        detail,
        missing_required,
        active: has_all && !disabled,
    }
}

fn api_server_platform(def: &GatewayPlatformDef, config: &AppConfig) -> PlatformDiagnostic {
    let enabled_env = std::env::var("API_SERVER_ENABLED").ok();
    let enabled_value = enabled_env.as_deref().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    });
    let explicitly_disabled = config.gateway.platform_disabled(def.id);
    let any_present = def.env_fields.iter().any(|field| env_is_set(field.key));
    let state = if enabled_value && !explicitly_disabled {
        PlatformState::Ready
    } else if enabled_value && explicitly_disabled {
        PlatformState::Available
    } else if any_present || config.gateway.platform_enabled(def.id) {
        PlatformState::Incomplete
    } else {
        PlatformState::NotConfigured
    };
    let detail = if enabled_value && !explicitly_disabled {
        "OpenAI-compatible API enabled".to_string()
    } else if enabled_value && explicitly_disabled {
        "API server configured but disabled".to_string()
    } else if any_present {
        "set API_SERVER_ENABLED=true to activate".to_string()
    } else if explicitly_disabled {
        "disabled in config".to_string()
    } else {
        "not configured yet".to_string()
    };

    PlatformDiagnostic {
        id: def.id,
        name: def.name,
        description: def.description,
        setup_kind: def.setup_kind,
        state,
        detail,
        missing_required: if enabled_value || !any_present {
            Vec::new()
        } else {
            vec!["API_SERVER_ENABLED"]
        },
        active: enabled_value && !explicitly_disabled,
    }
}

fn generic_env_platform(def: &GatewayPlatformDef, config: &AppConfig) -> PlatformDiagnostic {
    let required_fields: Vec<&EnvField> = def
        .env_fields
        .iter()
        .filter(|field| field.required)
        .collect();
    let present_required: Vec<(&'static str, bool)> = required_fields
        .iter()
        .map(|field| (field.key, env_is_set(field.key)))
        .collect();
    let missing_required = required_missing(&present_required);
    let any_present = def.env_fields.iter().any(|field| env_is_set(field.key));
    let explicitly_enabled = config.gateway.platform_enabled(def.id);
    let explicitly_disabled = config.gateway.platform_disabled(def.id);
    let has_all = !required_fields.is_empty() && missing_required.is_empty();
    let state = if explicitly_disabled && has_all {
        PlatformState::Available
    } else if has_all {
        PlatformState::Ready
    } else if explicitly_enabled || any_present {
        PlatformState::Incomplete
    } else {
        PlatformState::NotConfigured
    };
    let detail = if explicitly_disabled && has_all {
        "credentials present but disabled".to_string()
    } else if has_all {
        format!("{} required values present", required_fields.len())
    } else if explicitly_disabled {
        "disabled in config".to_string()
    } else if !any_present && !explicitly_enabled {
        "not configured yet".to_string()
    } else if !missing_required.is_empty() {
        format!("missing {}", missing_required.join(", "))
    } else {
        "not configured yet".to_string()
    };

    PlatformDiagnostic {
        id: def.id,
        name: def.name,
        description: def.description,
        setup_kind: def.setup_kind,
        state,
        detail,
        missing_required,
        active: has_all && !explicitly_disabled,
    }
}

fn required_missing(fields: &[(&'static str, bool)]) -> Vec<&'static str> {
    fields
        .iter()
        .filter_map(|(key, present)| if *present { None } else { Some(*key) })
        .collect()
}

fn env_is_set(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|value| not_blank(&value))
}

fn not_blank(value: &str) -> bool {
    !value.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn platform_catalog_lists_all_gateway_paths() {
        let ids: Vec<&str> = all_platforms().iter().map(|platform| platform.id).collect();
        assert_eq!(
            ids,
            vec![
                "telegram",
                "discord",
                "slack",
                "feishu",
                "wecom",
                "signal",
                "whatsapp",
                "webhook",
                "email",
                "sms",
                "matrix",
                "mattermost",
                "dingtalk",
                "homeassistant",
                "api_server",
            ]
        );
    }

    #[test]
    fn generic_env_platform_detects_partial_configuration() {
        let _guard = lock_test_env();
        let config = AppConfig::default();
        unsafe {
            std::env::set_var("TWILIO_ACCOUNT_SID", "sid");
            std::env::remove_var("TWILIO_AUTH_TOKEN");
            std::env::remove_var("TWILIO_PHONE_NUMBER");
        }

        let diag = diagnose_platform(find_platform("sms").unwrap(), &config);
        assert_eq!(diag.state, PlatformState::Incomplete);
        assert_eq!(
            diag.missing_required,
            vec!["TWILIO_AUTH_TOKEN", "TWILIO_PHONE_NUMBER"]
        );

        unsafe {
            std::env::remove_var("TWILIO_ACCOUNT_SID");
            std::env::remove_var("TWILIO_AUTH_TOKEN");
            std::env::remove_var("TWILIO_PHONE_NUMBER");
        }
    }

    #[test]
    fn whatsapp_platform_detects_unpaired_session() {
        let _guard = lock_test_env();
        let temp = tempdir().expect("temp dir");
        let mut config = AppConfig::default();
        config.gateway.whatsapp.enabled = true;
        config.gateway.whatsapp.session_path = Some(temp.path().join("whatsapp"));

        let diag = diagnose_platform(find_platform("whatsapp").unwrap(), &config);
        assert_eq!(diag.state, PlatformState::Incomplete);
        assert!(!diag.active);
    }

    #[test]
    fn generic_platform_not_configured_does_not_emit_missing_detail() {
        let _guard = lock_test_env();
        unsafe {
            std::env::remove_var("MATTERMOST_URL");
            std::env::remove_var("MATTERMOST_TOKEN");
        }
        let config = AppConfig::default();
        let diag = diagnose_platform(find_platform("mattermost").unwrap(), &config);
        assert_eq!(diag.state, PlatformState::NotConfigured);
        assert_eq!(diag.detail, "not configured yet");
    }

    #[test]
    fn email_platform_requires_domain_for_mailgun() {
        let _guard = lock_test_env();
        unsafe {
            std::env::set_var("EMAIL_PROVIDER", "mailgun");
            std::env::set_var("EMAIL_API_KEY", "key");
            std::env::set_var("EMAIL_FROM", "bot@example.com");
            std::env::remove_var("EMAIL_DOMAIN");
        }
        let config = AppConfig::default();
        let diag = diagnose_platform(find_platform("email").unwrap(), &config);
        assert_eq!(diag.state, PlatformState::Incomplete);
        assert!(diag.missing_required.contains(&"EMAIL_DOMAIN"));
        unsafe {
            std::env::remove_var("EMAIL_PROVIDER");
            std::env::remove_var("EMAIL_API_KEY");
            std::env::remove_var("EMAIL_FROM");
        }
    }

    #[test]
    fn email_platform_accepts_generic_smtp_password_without_api_key() {
        let _guard = lock_test_env();
        unsafe {
            std::env::set_var("EMAIL_PROVIDER", "generic_smtp");
            std::env::set_var("EMAIL_FROM", "bot@example.com");
            std::env::set_var("EMAIL_SMTP_HOST", "smtp.example.com");
            std::env::set_var("EMAIL_SMTP_PASSWORD", "secret");
            std::env::remove_var("EMAIL_API_KEY");
        }
        let config = AppConfig::default();
        let diag = diagnose_platform(find_platform("email").unwrap(), &config);
        assert_eq!(diag.state, PlatformState::Ready);
        assert!(diag.missing_required.is_empty());
        unsafe {
            std::env::remove_var("EMAIL_PROVIDER");
            std::env::remove_var("EMAIL_FROM");
            std::env::remove_var("EMAIL_SMTP_HOST");
            std::env::remove_var("EMAIL_SMTP_PASSWORD");
        }
    }

    #[test]
    fn api_server_false_requires_explicit_enable() {
        let _guard = lock_test_env();
        unsafe {
            std::env::set_var("API_SERVER_ENABLED", "false");
        }
        let config = AppConfig::default();
        let diag = diagnose_platform(find_platform("api_server").unwrap(), &config);
        assert_eq!(diag.state, PlatformState::Incomplete);
        assert_eq!(diag.detail, "set API_SERVER_ENABLED=true to activate");
        unsafe {
            std::env::remove_var("API_SERVER_ENABLED");
        }
    }

    #[test]
    fn explicit_disable_overrides_legacy_typed_enable_in_diagnostics() {
        let _guard = lock_test_env();
        unsafe {
            std::env::set_var("TELEGRAM_BOT_TOKEN", "token");
        }
        let mut config = AppConfig::default();
        config.gateway.telegram.enabled = true;
        config.gateway.disable_platform("telegram");

        let diag = diagnose_platform(find_platform("telegram").unwrap(), &config);
        assert_eq!(diag.state, PlatformState::Available);
        assert_eq!(diag.detail, "bot token present");
        assert!(!diag.active);

        unsafe {
            std::env::remove_var("TELEGRAM_BOT_TOKEN");
        }
    }
}
