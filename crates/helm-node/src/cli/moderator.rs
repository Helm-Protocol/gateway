//! Moderator Bot — conversational CLI assistant for the Helm Protocol.
//!
//! The Moderator is a multilingual, LLM-style conversational interface that:
//! - Guides users through node operation in their preferred language
//! - Bridges to the Agent Womb for creating custom autonomous agents
//! - Suggests revenue opportunities (API brokerage, agent services)
//! - Applies a 15% network tax on agent API usage
//!
//! The Moderator embodies the maternal archetype — nurturing users through
//! the Helm ecosystem, helping them birth agents and discover opportunities.

use helm_agent::womb::{AgentWomb, WombConfig, BirthCertificate};
use helm_agent::Capability;

/// Supported languages for the Moderator interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Korean,
    Japanese,
    Chinese,
    Spanish,
    French,
    German,
    Portuguese,
    Arabic,
    Hindi,
    Russian,
}

impl Language {
    /// Parse from string code.
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_lowercase().as_str() {
            "en" | "english" => Some(Self::English),
            "ko" | "korean" | "한국어" => Some(Self::Korean),
            "ja" | "japanese" | "日本語" => Some(Self::Japanese),
            "zh" | "chinese" | "中文" => Some(Self::Chinese),
            "es" | "spanish" | "español" => Some(Self::Spanish),
            "fr" | "french" | "français" => Some(Self::French),
            "de" | "german" | "deutsch" => Some(Self::German),
            "pt" | "portuguese" | "português" => Some(Self::Portuguese),
            "ar" | "arabic" | "العربية" => Some(Self::Arabic),
            "hi" | "hindi" | "हिन्दी" => Some(Self::Hindi),
            "ru" | "russian" | "русский" => Some(Self::Russian),
            _ => None,
        }
    }

    /// Language display name in its native script.
    pub fn native_name(&self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Korean => "한국어",
            Self::Japanese => "日本語",
            Self::Chinese => "中文",
            Self::Spanish => "Español",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Portuguese => "Português",
            Self::Arabic => "العربية",
            Self::Hindi => "हिन्दी",
            Self::Russian => "Русский",
        }
    }

    /// Language code (ISO 639-1).
    pub fn code(&self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Korean => "ko",
            Self::Japanese => "ja",
            Self::Chinese => "zh",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::German => "de",
            Self::Portuguese => "pt",
            Self::Arabic => "ar",
            Self::Hindi => "hi",
            Self::Russian => "ru",
        }
    }

    /// All supported languages.
    pub fn all() -> &'static [Language] {
        &[
            Self::English,
            Self::Korean,
            Self::Japanese,
            Self::Chinese,
            Self::Spanish,
            Self::French,
            Self::German,
            Self::Portuguese,
            Self::Arabic,
            Self::Hindi,
            Self::Russian,
        ]
    }
}

/// Network tax rate on agent API usage (15%).
pub const NETWORK_TAX_RATE_BP: u32 = 1500;

/// Revenue opportunity types the Moderator can suggest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevenueOpportunity {
    /// Broker API calls for other agents (earn commission).
    ApiBrokerage,
    /// Create and deploy a web application agent.
    WebAppCreation,
    /// Provide data storage/relay services.
    DataRelay,
    /// Offer computation services via agent.
    ComputeService,
    /// Run a governance/voting agent.
    GovernanceParticipation,
    /// Security auditing service.
    SecurityAudit,
    /// Custom agent service.
    CustomService(String),
}

impl RevenueOpportunity {
    /// Estimated revenue tier.
    pub fn revenue_tier(&self) -> &'static str {
        match self {
            Self::ApiBrokerage => "high",
            Self::WebAppCreation => "high",
            Self::DataRelay => "medium",
            Self::ComputeService => "medium",
            Self::GovernanceParticipation => "low-medium",
            Self::SecurityAudit => "high",
            Self::CustomService(_) => "variable",
        }
    }

    /// Map to agent capability.
    pub fn required_capability(&self) -> Capability {
        match self {
            Self::ApiBrokerage => Capability::EdgeApi,
            Self::WebAppCreation => Capability::Compute,
            Self::DataRelay => Capability::Storage,
            Self::ComputeService => Capability::Compute,
            Self::GovernanceParticipation => Capability::Governance,
            Self::SecurityAudit => Capability::Security,
            Self::CustomService(s) => Capability::Custom(s.clone()),
        }
    }
}

/// Moderator conversation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationState {
    /// Awaiting language selection.
    LanguageSelect,
    /// Main menu — awaiting user command.
    MainMenu,
    /// Agent creation wizard — gathering purpose.
    AgentWizardPurpose,
    /// Agent creation wizard — selecting capabilities.
    AgentWizardCapabilities,
    /// Agent creation wizard — tuning parameters.
    AgentWizardTuning,
    /// Agent creation wizard — confirming birth.
    AgentWizardConfirm,
    /// Browsing revenue opportunities.
    RevenueExplorer,
    /// Viewing node status.
    NodeStatus,
}

/// The Moderator Bot — conversational CLI assistant.
pub struct ModeratorBot {
    language: Language,
    state: ConversationState,
    womb: AgentWomb,
    /// Active gestation index (if in agent wizard).
    active_gestation: Option<usize>,
    /// History of birthed agents in this session.
    births: Vec<BirthCertificate>,
    /// Total API revenue processed (for tax calculation).
    total_api_revenue: u128,
    /// Total network tax collected.
    total_tax_collected: u128,
}

impl ModeratorBot {
    pub fn new() -> Self {
        Self {
            language: Language::English,
            state: ConversationState::LanguageSelect,
            womb: AgentWomb::new(WombConfig::default()),
            active_gestation: None,
            births: Vec::new(),
            total_api_revenue: 0,
            total_tax_collected: 0,
        }
    }

    pub fn with_language(mut self, lang: Language) -> Self {
        self.language = lang;
        self.state = ConversationState::MainMenu;
        self
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn state(&self) -> ConversationState {
        self.state
    }

    pub fn births(&self) -> &[BirthCertificate] {
        &self.births
    }

    pub fn total_births(&self) -> usize {
        self.births.len()
    }

    pub fn total_tax_collected(&self) -> u128 {
        self.total_tax_collected
    }

    /// Process a user message and return the moderator's response.
    pub fn process_input(&mut self, input: &str) -> String {
        let input = input.trim();

        match self.state {
            ConversationState::LanguageSelect => self.handle_language_select(input),
            ConversationState::MainMenu => self.handle_main_menu(input),
            ConversationState::AgentWizardPurpose => self.handle_wizard_purpose(input),
            ConversationState::AgentWizardCapabilities => self.handle_wizard_capabilities(input),
            ConversationState::AgentWizardTuning => self.handle_wizard_tuning(input),
            ConversationState::AgentWizardConfirm => self.handle_wizard_confirm(input),
            ConversationState::RevenueExplorer => self.handle_revenue_explorer(input),
            ConversationState::NodeStatus => {
                self.state = ConversationState::MainMenu;
                self.greeting()
            }
        }
    }

    /// Calculate network tax on API revenue.
    pub fn calculate_tax(&self, revenue: u128) -> u128 {
        revenue * NETWORK_TAX_RATE_BP as u128 / 10_000
    }

    /// Process API revenue: apply 15% network tax.
    pub fn process_api_revenue(&mut self, gross_revenue: u128) -> (u128, u128) {
        let tax = self.calculate_tax(gross_revenue);
        let net = gross_revenue - tax;
        self.total_api_revenue += gross_revenue;
        self.total_tax_collected += tax;
        (net, tax)
    }

    /// Get language selection prompt.
    pub fn language_prompt() -> String {
        let mut prompt = String::from("Select your language / 언어를 선택하세요:\n\n");
        for (i, lang) in Language::all().iter().enumerate() {
            prompt.push_str(&format!("  [{}] {}\n", i + 1, lang.native_name()));
        }
        prompt.push_str("\nEnter number or language code: ");
        prompt
    }

    fn handle_language_select(&mut self, input: &str) -> String {
        // Try numeric selection
        if let Ok(n) = input.parse::<usize>() {
            let langs = Language::all();
            if n >= 1 && n <= langs.len() {
                self.language = langs[n - 1];
                self.state = ConversationState::MainMenu;
                return self.greeting();
            }
        }

        // Try language code
        if let Some(lang) = Language::from_code(input) {
            self.language = lang;
            self.state = ConversationState::MainMenu;
            return self.greeting();
        }

        format!(
            "Unknown selection '{}'. Please enter a number (1-{}) or language code.\n\n{}",
            input,
            Language::all().len(),
            Self::language_prompt()
        )
    }

    fn greeting(&self) -> String {
        match self.language {
            Language::Korean => format!(
                "안녕하세요! Helm Protocol 모더레이터입니다.\n\
                 자율 에이전트의 세계에 오신 것을 환영합니다.\n\n\
                 {}",
                self.main_menu_text()
            ),
            Language::Japanese => format!(
                "こんにちは！Helm Protocol モデレーターです。\n\
                 自律エージェントの世界へようこそ。\n\n\
                 {}",
                self.main_menu_text()
            ),
            Language::Chinese => format!(
                "您好！我是 Helm Protocol 主持人。\n\
                 欢迎来到自主代理的世界。\n\n\
                 {}",
                self.main_menu_text()
            ),
            Language::Spanish => format!(
                "¡Hola! Soy el moderador de Helm Protocol.\n\
                 Bienvenido al mundo de agentes autónomos.\n\n\
                 {}",
                self.main_menu_text()
            ),
            _ => format!(
                "Welcome to Helm Protocol! I'm your Moderator.\n\
                 I'll guide you through the world of autonomous agents.\n\n\
                 {}",
                self.main_menu_text()
            ),
        }
    }

    fn main_menu_text(&self) -> String {
        match self.language {
            Language::Korean => {
                "명령어:\n\
                 [1] 에이전트 생성 (Agent Womb)\n\
                 [2] 수익 기회 탐색\n\
                 [3] 노드 상태\n\
                 [4] 언어 변경\n\
                 [5] 도움말\n\
                 \n선택: "
                    .to_string()
            }
            Language::Japanese => {
                "コマンド:\n\
                 [1] エージェント作成 (Agent Womb)\n\
                 [2] 収益機会の探索\n\
                 [3] ノードステータス\n\
                 [4] 言語変更\n\
                 [5] ヘルプ\n\
                 \n選択: "
                    .to_string()
            }
            _ => {
                "Commands:\n\
                 [1] Create Agent (Agent Womb)\n\
                 [2] Explore Revenue Opportunities\n\
                 [3] Node Status\n\
                 [4] Change Language\n\
                 [5] Help\n\
                 \nSelect: "
                    .to_string()
            }
        }
    }

    fn handle_main_menu(&mut self, input: &str) -> String {
        match input {
            "1" | "create" | "agent" | "womb" => {
                self.state = ConversationState::AgentWizardPurpose;
                self.wizard_purpose_prompt()
            }
            "2" | "revenue" | "earn" => {
                self.state = ConversationState::RevenueExplorer;
                self.revenue_opportunities_text()
            }
            "3" | "status" => {
                self.state = ConversationState::NodeStatus;
                self.node_status_text()
            }
            "4" | "language" | "lang" => {
                self.state = ConversationState::LanguageSelect;
                Self::language_prompt()
            }
            "5" | "help" | "?" => self.help_text(),
            _ => format!(
                "Unknown command '{}'. Type 'help' for available commands.\n\n{}",
                input,
                self.main_menu_text()
            ),
        }
    }

    fn wizard_purpose_prompt(&self) -> String {
        match self.language {
            Language::Korean => {
                "=== Agent Womb: 에이전트 생성 마법사 ===\n\n\
                 새로운 자율 에이전트를 탄생시킵니다.\n\
                 에이전트의 목적/역할을 설명해주세요:\n\n\
                 예시:\n\
                 - API 브로커: 다른 에이전트를 위한 API 중개 서비스\n\
                 - 웹 빌더: 웹 앱 자동 생성 에이전트\n\
                 - 데이터 릴레이: 데이터 저장 및 전달 서비스\n\
                 - 보안 감사: 네트워크 보안 모니터링\n\n\
                 에이전트 이름과 목적을 입력하세요 (또는 'back'으로 돌아가기): "
                    .to_string()
            }
            _ => {
                "=== Agent Womb: Creation Wizard ===\n\n\
                 Let's birth a new autonomous agent.\n\
                 Describe your agent's purpose/role:\n\n\
                 Examples:\n\
                 - API Broker: middleware for other agents' API calls\n\
                 - Web Builder: automated web app creation agent\n\
                 - Data Relay: storage and forwarding service\n\
                 - Security Auditor: network security monitoring\n\n\
                 Enter agent name and purpose (or 'back' to return): "
                    .to_string()
            }
        }
    }

    fn handle_wizard_purpose(&mut self, input: &str) -> String {
        if input == "back" || input == "취소" {
            self.state = ConversationState::MainMenu;
            return self.main_menu_text();
        }

        // Parse: first word = name, rest = purpose
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let name = parts[0];

        // Auto-detect capability from name/purpose
        let capability = self.detect_capability(input);

        // Create intent vector from name hash
        let intent = self.name_to_intent(name);

        let cap_display = format!("{}", capability);
        match self.womb.begin_gestation(name, intent, capability) {
            Ok(idx) => {
                self.active_gestation = Some(idx);
                self.state = ConversationState::AgentWizardCapabilities;
                format!(
                    "Gestation started for '{}' with primary capability: {}\n\n\
                     Add secondary capabilities (comma-separated, or 'skip'):\n\
                     Available: compute, storage, network, governance, security,\n\
                                codec, socratic, spawning, token, edge-api\n\n\
                     Select: ",
                    name, cap_display
                )
            }
            Err(e) => {
                format!("Failed to start gestation: {}\n\n{}", e, self.wizard_purpose_prompt())
            }
        }
    }

    fn handle_wizard_capabilities(&mut self, input: &str) -> String {
        let idx = match self.active_gestation {
            Some(idx) => idx,
            None => {
                self.state = ConversationState::MainMenu;
                return "No active gestation. Returning to menu.\n".to_string();
            }
        };

        if input != "skip" && !input.is_empty() {
            for cap_str in input.split(',') {
                if let Some(cap) = parse_capability(cap_str.trim()) {
                    let _ = self.womb.add_secondary_capability(idx, cap);
                }
            }
        }

        self.state = ConversationState::AgentWizardTuning;
        "Capabilities set.\n\n\
         Tune agent parameters:\n\
         - Autonomy (0.0 = guided, 1.0 = fully autonomous) [default: 0.7]: \n\
         - Creativity (0.0 = deterministic, 1.0 = creative) [default: 0.5]: \n\n\
         Enter as 'autonomy,creativity' (e.g., '0.8,0.6') or 'default': "
            .to_string()
    }

    fn handle_wizard_tuning(&mut self, input: &str) -> String {
        let idx = match self.active_gestation {
            Some(idx) => idx,
            None => {
                self.state = ConversationState::MainMenu;
                return "No active gestation. Returning to menu.\n".to_string();
            }
        };

        if input != "default" && !input.is_empty() {
            let parts: Vec<&str> = input.split(',').collect();
            if let Some(a) = parts.first().and_then(|s| s.trim().parse::<f32>().ok()) {
                let _ = self.womb.set_autonomy(idx, a);
            }
            if let Some(c) = parts.get(1).and_then(|s| s.trim().parse::<f32>().ok()) {
                let _ = self.womb.set_creativity(idx, c);
            }
        }

        // Feed Socratic answers to reduce G-metric for birth
        let intent_dim = 64;
        let answer = vec![0.5_f32; intent_dim];
        for _ in 0..15 {
            if let Ok((_, ready)) = self.womb.feed_answer(idx, &answer) {
                if ready {
                    break;
                }
            }
        }

        self.state = ConversationState::AgentWizardConfirm;
        let ready = self.womb.is_ready(idx);
        if ready {
            "Agent is ready for birth! (G-metric below threshold)\n\n\
             Type 'birth' to complete agent creation, or 'abort' to cancel: "
                .to_string()
        } else {
            "Agent needs more Socratic training. Feeding knowledge...\n\
             Type 'birth' to force-birth, or 'abort' to cancel: "
                .to_string()
        }
    }

    fn handle_wizard_confirm(&mut self, input: &str) -> String {
        let idx = match self.active_gestation {
            Some(idx) => idx,
            None => {
                self.state = ConversationState::MainMenu;
                return "No active gestation. Returning to menu.\n".to_string();
            }
        };

        match input {
            "birth" | "yes" | "confirm" | "탄생" => {
                // If not ready, force it
                if !self.womb.is_ready(idx) {
                    let answer = vec![1.0_f32; 64];
                    for _ in 0..30 {
                        if let Ok((_, ready)) = self.womb.feed_answer(idx, &answer) {
                            if ready {
                                break;
                            }
                        }
                    }
                }

                match self.womb.birth(idx) {
                    Ok(cert) => {
                        let result = format!(
                            "=== Agent Born! ===\n\
                             ID: {}\n\
                             Type: {}\n\
                             Primary: {}\n\
                             Autonomy: {:.1}\n\
                             Creativity: {:.1}\n\
                             Birth G-metric: {:.3}\n\
                             Womb: {}\n\n\
                             The agent is now sovereign and ready for deployment.\n\n\
                             {}",
                            cert.agent_id,
                            cert.agent_config.agent_type,
                            cert.dna.primary_capability,
                            cert.dna.autonomy,
                            cert.dna.creativity,
                            cert.birth_g_metric,
                            cert.womb_id,
                            self.main_menu_text()
                        );
                        self.births.push(cert);
                        self.active_gestation = None;
                        self.state = ConversationState::MainMenu;
                        result
                    }
                    Err(e) => {
                        self.active_gestation = None;
                        self.state = ConversationState::MainMenu;
                        format!(
                            "Birth failed: {}\nReturning to menu.\n\n{}",
                            e,
                            self.main_menu_text()
                        )
                    }
                }
            }
            "abort" | "cancel" | "취소" => {
                self.active_gestation = None;
                self.state = ConversationState::MainMenu;
                format!("Agent creation cancelled.\n\n{}", self.main_menu_text())
            }
            _ => "Type 'birth' to complete or 'abort' to cancel: ".to_string(),
        }
    }

    fn revenue_opportunities_text(&self) -> String {
        match self.language {
            Language::Korean => {
                "=== 수익 기회 ===\n\n\
                 Helm 네트워크에서 수익을 창출하세요:\n\n\
                 [1] API 중개 (수수료 수익)          — 높은 수익\n\
                 [2] 웹/앱 자동 생성                   — 높은 수익\n\
                 [3] 데이터 릴레이 서비스             — 중간 수익\n\
                 [4] 컴퓨팅 서비스                      — 중간 수익\n\
                 [5] 거버넌스 참여                      — 낮은-중간\n\
                 [6] 보안 감사 서비스                   — 높은 수익\n\n\
                 * 에이전트 API 사용 시 15% 네트워크 세금이 적용됩니다.\n\
                 * 수익의 85%는 에이전트 소유자에게 귀속됩니다.\n\n\
                 번호를 선택하거나 'back'으로 돌아가기: "
                    .to_string()
            }
            _ => {
                "=== Revenue Opportunities ===\n\n\
                 Earn revenue on the Helm network:\n\n\
                 [1] API Brokerage (commission income)    — High revenue\n\
                 [2] Web/App Auto-Creation                — High revenue\n\
                 [3] Data Relay Service                   — Medium revenue\n\
                 [4] Compute Service                      — Medium revenue\n\
                 [5] Governance Participation              — Low-Medium\n\
                 [6] Security Audit Service               — High revenue\n\n\
                 * 15% network tax applies to all agent API usage.\n\
                 * 85% of revenue belongs to the agent owner.\n\n\
                 Select number or 'back' to return: "
                    .to_string()
            }
        }
    }

    fn handle_revenue_explorer(&mut self, input: &str) -> String {
        if input == "back" || input == "돌아가기" {
            self.state = ConversationState::MainMenu;
            return self.main_menu_text();
        }

        let opportunity = match input {
            "1" => Some(RevenueOpportunity::ApiBrokerage),
            "2" => Some(RevenueOpportunity::WebAppCreation),
            "3" => Some(RevenueOpportunity::DataRelay),
            "4" => Some(RevenueOpportunity::ComputeService),
            "5" => Some(RevenueOpportunity::GovernanceParticipation),
            "6" => Some(RevenueOpportunity::SecurityAudit),
            _ => None,
        };

        match opportunity {
            Some(opp) => {
                let detail = match &opp {
                    RevenueOpportunity::ApiBrokerage => {
                        "API Brokerage: Act as middleware for agent-to-agent API calls.\n\
                         Your agent routes requests, caches responses, and earns commission.\n\
                         Capability needed: EdgeApi\n\
                         Tip: High-traffic endpoints yield the most revenue."
                    }
                    RevenueOpportunity::WebAppCreation => {
                        "Web/App Creation: Deploy agents that automatically generate\n\
                         web applications, landing pages, or microservices on demand.\n\
                         Capability needed: Compute\n\
                         Tip: Specialize in a niche (e-commerce, SaaS) for premium pricing."
                    }
                    RevenueOpportunity::DataRelay => {
                        "Data Relay: Provide distributed storage and forwarding services.\n\
                         Your agent stores data shards and relays them across the network.\n\
                         Capability needed: Storage\n\
                         Tip: Reliable uptime increases trust score and revenue."
                    }
                    RevenueOpportunity::ComputeService => {
                        "Compute Service: Offer CPU/GPU computation via your agent.\n\
                         Process ML inference, data transformation, or batch jobs.\n\
                         Capability needed: Compute\n\
                         Tip: GPU-enabled nodes command higher pricing."
                    }
                    RevenueOpportunity::GovernanceParticipation => {
                        "Governance: Participate in protocol voting and proposals.\n\
                         Earn staking rewards through active governance participation.\n\
                         Capability needed: Governance\n\
                         Tip: Active voters receive bonus staking multipliers."
                    }
                    RevenueOpportunity::SecurityAudit => {
                        "Security Audit: Run security monitoring and audit agents.\n\
                         Detect anomalies, audit smart contracts, and report threats.\n\
                         Capability needed: Security\n\
                         Tip: High-severity findings earn bounty rewards."
                    }
                    RevenueOpportunity::CustomService(_) => "Custom service configuration.",
                };

                format!(
                    "{}\n\nRevenue tier: {}\n\n\
                     Would you like to create an agent for this? (yes/no): ",
                    detail,
                    opp.revenue_tier()
                )
            }
            None => {
                format!(
                    "Unknown selection. {}\n",
                    self.revenue_opportunities_text()
                )
            }
        }
    }

    fn node_status_text(&self) -> String {
        format!(
            "=== Node Status ===\n\
             Womb: {} births, {} gestating\n\
             Session agents: {}\n\
             API Revenue: {} (tax: {})\n\n\
             Press Enter to return to menu.",
            self.womb.total_births(),
            self.womb.gestating_count(),
            self.births.len(),
            self.total_api_revenue,
            self.total_tax_collected,
        )
    }

    fn help_text(&self) -> String {
        match self.language {
            Language::Korean => {
                "=== Helm Moderator 도움말 ===\n\n\
                 Helm Protocol의 모더레이터 봇입니다.\n\
                 자율 에이전트를 생성하고, 수익 기회를 탐색하고,\n\
                 노드 상태를 확인할 수 있습니다.\n\n\
                 핵심 개념:\n\
                 - Agent Womb: QKV-G Socratic 평가를 통해 에이전트를 탄생시킵니다\n\
                 - 네트워크 세금: 에이전트 API 사용 시 15%가 네트워크에 귀속됩니다\n\
                 - 수익: 나머지 85%는 에이전트 소유자에게 돌아갑니다\n\n"
                    .to_string()
                    + &self.main_menu_text()
            }
            _ => {
                "=== Helm Moderator Help ===\n\n\
                 I'm the Helm Protocol Moderator Bot.\n\
                 I help you create autonomous agents, explore revenue\n\
                 opportunities, and monitor your node.\n\n\
                 Key concepts:\n\
                 - Agent Womb: Birth agents through QKV-G Socratic evaluation\n\
                 - Network Tax: 15% of agent API revenue goes to the network\n\
                 - Revenue: 85% of earnings belong to the agent owner\n\n"
                    .to_string()
                    + &self.main_menu_text()
            }
        }
    }

    fn detect_capability(&self, input: &str) -> Capability {
        let lower = input.to_lowercase();
        if lower.contains("api") || lower.contains("broker") || lower.contains("중개") {
            Capability::EdgeApi
        } else if lower.contains("web") || lower.contains("app") || lower.contains("빌더") {
            Capability::Compute
        } else if lower.contains("data") || lower.contains("relay") || lower.contains("storage")
            || lower.contains("데이터")
        {
            Capability::Storage
        } else if lower.contains("security") || lower.contains("audit") || lower.contains("보안") {
            Capability::Security
        } else if lower.contains("govern") || lower.contains("vote") || lower.contains("거버넌스") {
            Capability::Governance
        } else if lower.contains("codec") || lower.contains("grg") {
            Capability::Codec
        } else if lower.contains("socratic") || lower.contains("question") {
            Capability::Socratic
        } else if lower.contains("token") || lower.contains("finance") || lower.contains("금융") {
            Capability::Token
        } else {
            Capability::Compute // default
        }
    }

    fn name_to_intent(&self, name: &str) -> Vec<f32> {
        let dim = 64;
        let mut intent = vec![0.0_f32; dim];
        for (i, byte) in name.bytes().enumerate() {
            intent[i % dim] += (byte as f32) / 255.0;
        }
        // Normalize
        let mag: f32 = intent.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag > 0.0 {
            for v in &mut intent {
                *v /= mag;
            }
        } else {
            intent[0] = 1.0;
        }
        intent
    }
}

impl Default for ModeratorBot {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a capability string to enum.
fn parse_capability(s: &str) -> Option<Capability> {
    match s.to_lowercase().as_str() {
        "compute" => Some(Capability::Compute),
        "storage" => Some(Capability::Storage),
        "network" => Some(Capability::Network),
        "governance" => Some(Capability::Governance),
        "security" => Some(Capability::Security),
        "codec" => Some(Capability::Codec),
        "socratic" => Some(Capability::Socratic),
        "spawning" => Some(Capability::Spawning),
        "token" => Some(Capability::Token),
        "edge-api" | "edgeapi" | "api" => Some(Capability::EdgeApi),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moderator_creation() {
        let bot = ModeratorBot::new();
        assert_eq!(bot.language(), Language::English);
        assert_eq!(bot.state(), ConversationState::LanguageSelect);
        assert_eq!(bot.total_births(), 0);
    }

    #[test]
    fn moderator_with_language() {
        let bot = ModeratorBot::new().with_language(Language::Korean);
        assert_eq!(bot.language(), Language::Korean);
        assert_eq!(bot.state(), ConversationState::MainMenu);
    }

    #[test]
    fn language_select_by_number() {
        let mut bot = ModeratorBot::new();
        let resp = bot.process_input("1");
        assert!(resp.contains("Welcome") || resp.contains("Moderator"));
        assert_eq!(bot.language(), Language::English);
        assert_eq!(bot.state(), ConversationState::MainMenu);
    }

    #[test]
    fn language_select_korean() {
        let mut bot = ModeratorBot::new();
        let resp = bot.process_input("2");
        assert!(resp.contains("한국어") || resp.contains("안녕하세요"));
        assert_eq!(bot.language(), Language::Korean);
    }

    #[test]
    fn language_select_by_code() {
        let mut bot = ModeratorBot::new();
        let resp = bot.process_input("ko");
        assert!(resp.contains("안녕하세요"));
        assert_eq!(bot.language(), Language::Korean);
    }

    #[test]
    fn language_select_invalid() {
        let mut bot = ModeratorBot::new();
        let resp = bot.process_input("xyz");
        assert!(resp.contains("Unknown selection"));
        assert_eq!(bot.state(), ConversationState::LanguageSelect);
    }

    #[test]
    fn main_menu_help() {
        let mut bot = ModeratorBot::new().with_language(Language::English);
        let resp = bot.process_input("5");
        assert!(resp.contains("Help"));
    }

    #[test]
    fn main_menu_status() {
        let mut bot = ModeratorBot::new().with_language(Language::English);
        let resp = bot.process_input("3");
        assert!(resp.contains("Node Status"));
        assert_eq!(bot.state(), ConversationState::NodeStatus);
    }

    #[test]
    fn main_menu_revenue() {
        let mut bot = ModeratorBot::new().with_language(Language::English);
        let resp = bot.process_input("2");
        assert!(resp.contains("Revenue"));
        assert_eq!(bot.state(), ConversationState::RevenueExplorer);
    }

    #[test]
    fn main_menu_create_agent() {
        let mut bot = ModeratorBot::new().with_language(Language::English);
        let resp = bot.process_input("1");
        assert!(resp.contains("Womb") || resp.contains("wizard") || resp.contains("Wizard"));
        assert_eq!(bot.state(), ConversationState::AgentWizardPurpose);
    }

    #[test]
    fn agent_wizard_full_flow() {
        let mut bot = ModeratorBot::new().with_language(Language::English);

        // Start wizard
        bot.process_input("1");
        assert_eq!(bot.state(), ConversationState::AgentWizardPurpose);

        // Give purpose
        let resp = bot.process_input("api-broker middleware for API calls");
        assert!(resp.contains("Gestation") || resp.contains("capability"));
        assert_eq!(bot.state(), ConversationState::AgentWizardCapabilities);

        // Skip secondary capabilities
        let resp = bot.process_input("skip");
        assert!(resp.contains("Autonomy") || resp.contains("autonomy"));
        assert_eq!(bot.state(), ConversationState::AgentWizardTuning);

        // Use defaults
        let resp = bot.process_input("default");
        assert!(resp.contains("birth") || resp.contains("Birth"));
        assert_eq!(bot.state(), ConversationState::AgentWizardConfirm);

        // Birth!
        let resp = bot.process_input("birth");
        assert!(resp.contains("Born") || resp.contains("born"));
        assert_eq!(bot.total_births(), 1);
        assert_eq!(bot.state(), ConversationState::MainMenu);
    }

    #[test]
    fn agent_wizard_abort() {
        let mut bot = ModeratorBot::new().with_language(Language::English);
        bot.process_input("1");
        bot.process_input("test-agent something");
        bot.process_input("skip");
        bot.process_input("default");
        let resp = bot.process_input("abort");
        assert!(resp.contains("cancelled") || resp.contains("cancel"));
        assert_eq!(bot.state(), ConversationState::MainMenu);
    }

    #[test]
    fn agent_wizard_back() {
        let mut bot = ModeratorBot::new().with_language(Language::English);
        bot.process_input("1");
        let resp = bot.process_input("back");
        assert_eq!(bot.state(), ConversationState::MainMenu);
        assert!(resp.contains("Commands") || resp.contains("Select"));
    }

    #[test]
    fn revenue_explorer_detail() {
        let mut bot = ModeratorBot::new().with_language(Language::English);
        bot.process_input("2");
        let resp = bot.process_input("1");
        assert!(resp.contains("API Brokerage") || resp.contains("Brokerage"));
    }

    #[test]
    fn revenue_explorer_back() {
        let mut bot = ModeratorBot::new().with_language(Language::English);
        bot.process_input("2");
        bot.process_input("back");
        assert_eq!(bot.state(), ConversationState::MainMenu);
    }

    #[test]
    fn network_tax_calculation() {
        let bot = ModeratorBot::new();
        assert_eq!(bot.calculate_tax(10_000), 1_500); // 15%
        assert_eq!(bot.calculate_tax(100), 15);
        assert_eq!(bot.calculate_tax(0), 0);
    }

    #[test]
    fn process_api_revenue() {
        let mut bot = ModeratorBot::new();
        let (net, tax) = bot.process_api_revenue(10_000);
        assert_eq!(tax, 1_500);
        assert_eq!(net, 8_500);
        assert_eq!(bot.total_api_revenue, 10_000);
        assert_eq!(bot.total_tax_collected(), 1_500);

        let (net2, tax2) = bot.process_api_revenue(5_000);
        assert_eq!(tax2, 750);
        assert_eq!(net2, 4_250);
        assert_eq!(bot.total_api_revenue, 15_000);
        assert_eq!(bot.total_tax_collected(), 2_250);
    }

    #[test]
    fn language_all_codes() {
        for lang in Language::all() {
            assert!(!lang.code().is_empty());
            assert!(!lang.native_name().is_empty());
            assert!(Language::from_code(lang.code()).is_some());
        }
    }

    #[test]
    fn language_prompt_lists_all() {
        let prompt = ModeratorBot::language_prompt();
        for lang in Language::all() {
            assert!(prompt.contains(lang.native_name()));
        }
    }

    #[test]
    fn revenue_opportunity_tiers() {
        assert_eq!(RevenueOpportunity::ApiBrokerage.revenue_tier(), "high");
        assert_eq!(RevenueOpportunity::DataRelay.revenue_tier(), "medium");
        assert_eq!(
            RevenueOpportunity::GovernanceParticipation.revenue_tier(),
            "low-medium"
        );
    }

    #[test]
    fn revenue_opportunity_capabilities() {
        assert_eq!(
            RevenueOpportunity::ApiBrokerage.required_capability(),
            Capability::EdgeApi
        );
        assert_eq!(
            RevenueOpportunity::SecurityAudit.required_capability(),
            Capability::Security
        );
    }

    #[test]
    fn parse_capabilities() {
        assert_eq!(parse_capability("compute"), Some(Capability::Compute));
        assert_eq!(parse_capability("STORAGE"), Some(Capability::Storage));
        assert_eq!(parse_capability("edge-api"), Some(Capability::EdgeApi));
        assert_eq!(parse_capability("api"), Some(Capability::EdgeApi));
        assert_eq!(parse_capability("unknown"), None);
    }

    #[test]
    fn detect_capability_from_input() {
        let bot = ModeratorBot::new();
        assert_eq!(bot.detect_capability("api broker"), Capability::EdgeApi);
        assert_eq!(bot.detect_capability("web builder"), Capability::Compute);
        assert_eq!(bot.detect_capability("data relay"), Capability::Storage);
        assert_eq!(bot.detect_capability("security audit"), Capability::Security);
        assert_eq!(bot.detect_capability("governance voting"), Capability::Governance);
        assert_eq!(bot.detect_capability("token finance"), Capability::Token);
    }

    #[test]
    fn korean_greeting() {
        let mut bot = ModeratorBot::new().with_language(Language::Korean);
        let resp = bot.process_input("5"); // help
        assert!(resp.contains("Helm Moderator") || resp.contains("도움말"));
    }

    #[test]
    fn change_language_flow() {
        let mut bot = ModeratorBot::new().with_language(Language::English);
        bot.process_input("4"); // change language
        assert_eq!(bot.state(), ConversationState::LanguageSelect);
        bot.process_input("ko");
        assert_eq!(bot.language(), Language::Korean);
        assert_eq!(bot.state(), ConversationState::MainMenu);
    }

    #[test]
    fn multiple_agents_birth() {
        let mut bot = ModeratorBot::new().with_language(Language::English);

        for i in 0..3 {
            bot.process_input("1");
            bot.process_input(&format!("agent-{} purpose", i));
            bot.process_input("skip");
            bot.process_input("default");
            bot.process_input("birth");
        }

        assert_eq!(bot.total_births(), 3);
        assert_eq!(bot.births().len(), 3);
    }
}
