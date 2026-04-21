# Quantalogic – Deep Research Audit Document

**Date:** 2026-04-21  
**Prepared for:** Homelab profile analysis  
**Scope:** Company overview, legal status, leadership, products, technology, market positioning, and available financials.

---

## Executive Summary

Quantalogic is a French AI agent platform founded in July 2024, headquartered in Neuilly-sur‑Seine (Hauts‑de‑Seine). The company offers a sovereign, cloud‑agnostic AI framework enabling developers to build advanced agents that combine reasoning (ReAct) and code execution (CodeAct). Its flagship product, **QAtlas**, provides sovereign document intelligence using RAG vectorization coupled with a knowledge graph. Quantalogic emphasizes data sovereignty, European hosting (via OVHcloud), and a no‑code/low‑code approach for business users.

The company is in its very early stages, with a small team and no published financial statements. It describes itself as unfunded in private company databases, though maintains an active development and public presence.

---

## Company Information

| Item | Details |
|------|---------|
| **Legal Name** | QUANTALOGIC SAS |
| **SIREN** | 930 719 851 |
| **SIRET** | 930 719 851 00014 |
| **RCS** | Nanterre B 930 719 851 |
| **NAF/APE** | 6201Z – Computer programming activities |
| **Date of Incorporation** | 02 July 2024 |
| **Registered Office** | 171 ter Avenue Charles de Gaulle, Bâtiment D, 92200 Neuilly‑sur‑Seine, France |
| **Legal Form** | Société par actions simplifiée (SAS) |
| **Share Capital** | 5 000 € |
| **VAT ID** | FR 44 930719851 |
| **Website** | <https://www.quantalogic.app> |
| **LinkedIn** | <https://fr.linkedin.com/company/quantalogic> |
| **GitHub** | <https://github.com/quantalogic> |

*Sources: societe.com, annuaire-entreprises.data.gouv.fr, LinkedIn.*

---

## Leadership & Governance

| Name | Position | Background |
|------|----------|------------|
| **Raphaël MANSUY** | President (since 10 Jul 2024) | AI entrepreneur, French expat based in Hong Kong, advocate of sovereign AI. Involved in ventures such as StudentCentral.ai. Contributor to EdgeCrab open-source agent framework. |
| **Olivier LEFIEVRE** | General Director (since 10 Jul 2024) | Technical architect, previously CTO at Novagen Conseil. |
| **Hubert STEFANI** | General Director (since 10 Jul 2024) | Expert in AI platforms and partnerships. |

*Sources: societe.com, LinkedIn profiles.*

---

## Products & Services

### QuantaLogic AI Agent Platform
- **Core Framework**: Flexible AI framework for building agents that understand, reason, and execute complex tasks via natural language.
- **Paradigms Supported**:
  - **ReAct** (Reasoning & Action) – step‑by‑step reasoning with tool use.
  - **CodeAct** – agents generate and execute Python code as the primary action loop, iterating on results.
- **LLM Agnostic**: Integration with multiple providers (OpenAI, Anthropic, Mistral, DeepSeek, local/open‑source models).
- **Secure Tool System**: Uses Docker sandboxes for code execution and file manipulation.
- **Real‑time Monitoring**: Web UI for event visualization.
- **Memory Management**: Intelligent context handling and optimization.

### QAtlas – Sovereign Document Intelligence
- **Description**: Enterprise‑grade document search combining RAG vectorization and knowledge‑graph enrichment.
- **Key Claims**:
  - **Performance**: Engine designed for high speed.
  - **Precision**: Reduces noise and hallucinations via knowledge‑graph + vector coupling.
  - **Sovereignty**: Data remains exclusive property in compartmentalized environments.
  - **Contextual Understanding**: Maps relationships between entities for deeper relevance.

### Additional Offerings (from howto guide & platform)
- Web‑based ultra‑responsive multi‑LLM chat.
- Prompt & workflow creation, optimization, storage & sharing (“brain bank”).
- Autonomous agents connected to internal systems/applications.
- Deep‑Search agent for specialized research.
- 15 sovereign LLMs (French/open‑source hosted in France) available on request.
- Instant generation of high‑quality PowerPoints and images.
- No‑code/low‑code interface enabling business users to leverage AI without technical expertise.

*Sources: howto.md, quantalogic.app landing page, LinkedIn updates.*

---

## Technology Stack (inferred)

| Layer | Technology / Approach |
|-------|-----------------------|
| **Language** | Python (≥3.12) – primary implementation language |
| **Agent Framework** | Custom ReAct + CodeAct implementation |
| **LLM Integration** | Provider‑agnostic; supports OpenAI, Anthropic, Mistral, DeepSeek, local models |
| **Execution Sandbox** | Docker containers for secure code/file operations |
| **Orchestration** | Event‑driven architecture with observable hooks (task start/end, tool execution) |
| **Document Intelligence (QAtlas)** | Retrieval‑Augmented Generation (RAG) + Knowledge Graph |
| **Vector Store** | Likely FAISS, Qdrant, or similar (not explicitly stated) |
| **Frontend** | Web interface (likely React/Vue) for monitoring & chat |
| **Deployment** | Cloud‑agnostic; highlighted partnership with OVHcloud for European hosting |
| **DevOps** | pip/pipx installation; source install via git; testing framework (unspecified) |
| **Security** | Data compartmentalization, sovereign LLMs, no reliance on US hyperscalers (AWS/GCP/Azure) for core AI workloads |

*Sources: howto.md, LinkedIn post about Python sandbox, platform claims.*

---

## Market Position & Competitors

- **Industry**: AI agent platforms, enterprise AI, sovereign AI solutions.
- **Direct Competitors (per Tracxn)**:
  - Mistral AI (France)
  - Inflection (US)
  - Hyro (US)
  - Rasa (open‑source conversational AI)
  - Numerous other AI agent/framework startups.
- **Differentiators**:
  - Emphasis on **data sovereignty** and European hosting.
  - Dual ReAct/CodeAct paradigm offering flexibility.
  - Integrated sovereign document intelligence (QAtlas).
  - No‑code/low‑code accessibility for non‑technical users.
  - Multi‑LLM support avoiding vendor lock‑in.
- **Market Traction**:
  - 234 LinkedIn followers (as of April 2026).
  - Team size: 2‑10 employees (LinkedIn indicates ~5 engineers).
  - Participation in AI Leadership Tour by Scottish SME Delegation (British Embassy).
  - Early adopter messaging targeting startups, PMEs, ETIs, and large European enterprises seeking sovereign AI.

*Sources: Tracxn profile, LinkedIn, press mentions.*

---

## Partnerships & Ecosystem

- **OVHcloud**: Cited as hosting partner for the platform, providing cloud‑agnostic, European‑based infrastructure.
- **Novagen Conseil**: Founders’ prior affiliation; possible commercial or technical collaboration.
- **Open‑Source LLMs**: Platform claims to host and enrich 15 sovereign LLMs (French/open‑source) on demand.
- **Community**: Active GitHub repository (465 stars, 86 forks as of scraped date) indicating developer interest.

*Sources: landing page, LinkedIn, GitHub.*

---

## Financial Performance

- **Published Accounts**: As of the latest societe.com report, **no financial statements have been filed**; the company is listed as “Entreprise en défaut de publication de ses comptes (sauf exception)”.
- **Capital**: €5 000 share capital at incorporation.
- **Funding**: According to Tracxn and societe.com, QuantaLogic has **not raised any external funding rounds** (unfunded).
- **Revenue / Profit**: Not available in public filings.
- **Sovereignty Score**: 98/100 (per societe.com), indicating strong alignment with French/EU data‑sovereignty criteria.
- **Impact Score**: Not yet available.

*Sources: societe.com, Tracxn.*

---

## SWOT Analysis

### Strengths
- Strong founding team with AI and technical architecture background.
- Clear sovereign AI positioning appealing to European data‑privacy concerns.
- Dual ReAct/CodeAct framework offers flexibility for diverse use cases.
- Integrated document intelligence product (QAtlas) addressing a high‑value enterprise need.
- Partnership with OVHcloud ensures GDPR‑compliant hosting.
- No‑code/low‑code lowers adoption barrier for business units.

### Weaknesses
- Very early stage (founded 2024) – limited track record and customer references.
- No published financials; revenue model and traction unclear.
- Small team size may limit scalability of sales and support.
- Dependence on third‑party LLM providers for cutting‑edge models (though sovereign options offered).

### Opportunities
- Growing demand for sovereign AI solutions in EU (post‑Schrems II, AI Act).
- Expansion into regulated sectors (finance, healthcare, public administration) requiring data localization.
- Potential to monetize QAtlas as a standalone SaaS offering.
- Channel partnerships with SIEs and system integrators in France/EU.
- Leveraging open‑source LLMs to reduce dependency on costly proprietary APIs.

### Threats
- Intense competition from well‑funded US AI agent startups (e.g., Inflection, Adept) and established players (Microsoft Copilot, Google Vertex AI Agent Builder).
- Rapid evolution of LLM landscape could make current tech stack obsolete if not continuously updated.
- Regulatory changes (EU AI Act) may impose additional compliance burdens.
- Risk of talent attrition in competitive AI job market.

---

## Sources & References

1. **LinkedIn Company Page** – Quantalogic. <https://fr.linkedin.com/company/quantalogic>  
2. **Tracxn Company Profile** – QuantaLogic. <https://tracxn.com/d/companies/quantalogic/__PBbvh7zWyCEW1GY36wwVJj4w7GBfbldFSEA1X7JEdJU>  
3. **Societe.com** – Société QUANTALOGIC. <https://www.societe.com/societe/quantalogic-930719851.html>  
4. **Annuaire des Entreprises (INSEE)** – Quantalogic entry. <https://annuaire-entreprises.data.gouv.fr/entreprise/quantalogic-930719851>  
5. **Pappers.fr** – Company details (referenced in search results).  
6. **Quantalogic Official Website** – <https://www.quantalogic.app>  
7. **Quantalogic How‑to Guide** – GitHub. <https://github.com/quantalogic/quantalogic/blob/main/docs/howto/howto.md>  
8. **GitHub Repository** – <https://github.com/quantalogic/quantalogic>  
9. **Press/Mentions** – AI Leadership Tour, Scottish SME Delegation (LinkedIn posts).  
10. **OVHcloud Partnership** – Referenced in platform landing page and LinkedIn updates.  

---

**Disclaimer**: This document compiles publicly available information as of 21 April 2026. Financial data is limited due to non‑filing of accounts. For formal due‑diligence, direct engagement with the company and consultation of official registries (INPI, INSEE, tax authorities) is recommended.
