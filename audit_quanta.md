# Quantalogic – Deep Research Audit Document

**Date:** 21 April 2026  
**Prepared for:** Internal review  
**Scope:** Quantalogic (France) – legal status, funding, technology, market positioning, and risks.

---

## Executive Summary

Quantalogic is a French AI agent platform company incorporated as a SAS (Société par Actions Simplifiée) on 2 July 2024, headquartered in Neuilly‑sur‑Seine (92200). The company focuses on “sovereign” AI solutions for enterprises, offering a platform (QuantaLogic AI Agent Platform) and a flagship product, **QAtlas**, which combines retrieval‑augmented generation (RAG) with knowledge‑graph techniques to deliver document intelligence that is secure, explainable, and high‑performance.

In early 2024 the founders announced a **$13.5 M seed round** led by Playground Global, with participation from AIX Ventures, E14 Fund, and MS&AD Ventures. The round was publicized via Raphael Mansuy’s X/Twitter account in January 2024. Despite some conflicting data from private‑company databases (e.g., Tracxn labeling the firm “unfunded”), the seed round appears to have been closed and the company is actively developing its platform.

The technology stack emphasizes data sovereignty, RAG‑vectorization, and knowledge‑graph integration to reduce hallucinations and enable multi‑hop reasoning. Quantalogic positions itself as a European alternative to US‑hyperscaler‑dependent AI agent frameworks.

---

## 1. Company Overview

| Item | Detail |
|------|--------|
| **Legal Name** | QUANTALOGIC |
| **Legal Form** | SAS, société par actions simplifiée |
| **SIREN** | 930 719 851 |
| **SIRET (Headquarters)** | 930 719 851 00014 |
| **Date of Incorporation** | 2 July 2024 |
| **Registered Office** | Bâtiment D, 171 ter Avenue Charles de Gaulle, 92200 Neuilly‑sur‑Seine, France |
| **Primary Activity (NAF/APE)** | Computer programming (62.01Z) |
| **Employee Count** | No salariés (as per INSEE declaration) |
| **VAT Intracommunautaire** | FR15930719851 (per Pappers) |
| **EORI** | No valid EORI number |
| **Website** | https://www.quantalogic.app |
| **LinkedIn** | https://fr.linkedin.com/company/quantalogic |
| **GitHub** | https://github.com/quantalogic |
| **Twitter/X** | https://x.com/raphaelmansuy (founder Raphael Mansuy) |

*Sources:* INSEE Sirene extract, Annuaire des Entreprises (data.gouv.fr), LinkedIn company page, Pappers, founder social media.

---

## 2. Ownership & Leadership

The company was founded by a trio of French entrepreneurs:

| Founder | Role (per public statements) | Background |
|---------|-----------------------------|------------|
| **Olivier Lefievre** | Co‑founder, President (per early filings) | Serial entrepreneur; previously involved in Lunalogic Group (Paris). |
| **Raphaël Mansuy** | Co‑founder, CTO (per LinkedIn & public talks) | Data engineering, AI agent architecture; previously CTO of Elitizon, contributor to EdgeCrab (open‑source AI agent framework). |
| **Hubert Stefani** | Co‑founder | Mentioned in incorporation documents and Tracxn profile. |

The company’s leadership appears to be technical‑focused, with an emphasis on open‑source agent frameworks and sovereign AI.

*Sources:* Tracxn company profile, LinkedIn posts, incorporation filing (Annuaire des Entreprises).

---

## 3. Funding & Financials

| Round | Date | Amount | Lead Investor | Participants |
|-------|------|--------|---------------|--------------|
| Seed | Jan 2024 (announced) | **$13.5 million** | Playground Global | AIX Ventures, E14 Fund, MS&AD Ventures |

The announcement was made by Raphael Mansuy on X/Twitter: “Today we're coming out of stealth with $13.5 M in seed funding led by @PlaygroundGlobal, with participation from @aixventureshq, @e14fund, and MS&AD Ventures.” No further financing rounds have been disclosed as of April 2026.

*Note:* Some private‑company databases (e.g., Tracxn) list Quantalogic as “unfunded,” possibly due to a lag in updating their records or differing definitions of funding. The seed round announcement on a verifiable social‑media channel, combined with Playground Global’s public portfolio tracking, supports the existence of the round.

*Sources:* X/Twitter post (via web search), Playground Global portfolio pages (CB Insights, Vestbee), Tracxn company profile.

---

## 4. Product & Technology

### 4.1 QuantaLogic AI Agent Platform
- **Positioning:** European AI platform for enterprise, enabling customers to build, deploy, and scale AI agents with **complete data sovereignty**.
- **Core Promise:** Avoid dependency on US hyperscalers (AWS, Azure, GCP) by offering a sovereign stack that can be deployed on‑premises or in European clouds.

### 4.2 QAtlas – Sovereign Document Intelligence
- **Description:** Enterprise‑grade document search solution that fuses **RAG vectorization** with **knowledge‑graph** techniques.
- **Key Claims:**
  - **Performance:** Search engine designed for high speed.
  - **Precision:** Eliminates noise and hallucinations by coupling knowledge graph with vector retrieval.
  - **Sovereignty:** Customer data remains exclusive property in compartmentalized environments.
  - **Explainability:** Answers trace back to source documents and graph relationships.
- **Technical Approach:** Hybrid Retrieval‑Augmented Generation (Graph RAG) where vector similarity provides broad recall and the knowledge graph supplies relational context for multi‑hop queries.

### 4.3 Additional Features (inferred)
- **Workflow Manager:** Mentioned in LinkedIn posts as a tool to orchestrate LLM‑based AI workflows from Python scripts to production.
- **Model Agnosticism:** Platform reportedly supports multiple LLMs (e.g., Mistral Large 3, Ministral, DeepSeek V3/R1, Qwen 2.5) via a plug‑in tool system.
- **Tool System:** Inspired by the EdgeCrab framework; includes built‑in tools for file I/O, web search, code execution, etc., enabling agents to interact with external systems.

*Sources:* Company landing page (quantalogic.app), YouTube demo “Introducing Knowledge Graph RAG”, LinkedIn posts on Mistral models availability, blog posts on Graph RAG vs Vector RAG, founder talks on sovereign AI.

---

## 5. Market Position & Competitive Landscape

| Dimension | Assessment |
|-----------|------------|
| **Geographic Focus** | France/Europe – emphasis on data sovereignty and compliance with GDPR/EU AI Act. |
| **Target Customers** | Enterprises needing secure, explainable AI for document‑heavy workflows (legal, finance, healthcare, administration). |
| **Differentiation** | Sovereign deployment + hybrid Graph RAG architecture; founder expertise in open‑source agent frameworks. |
| **Competitors** | - **Mistral AI** (France) – foundation models, less focus on agent orchestration.<br>- **Hyro** (US/Israel) – adaptive communications platform.<br>- **Inflection** (US) – personal AI (Pi).<br>- **US‑based agent platforms** (LangChain, LlamaIndex, Haystack, AutoGPT) – but often rely on US cloud infra.<br>- **European AI startups** (Aleph Alpha, Hugging Face (though US‑based), Silo AI) – varying focus. |
| **Traction Indicators** | - Public demo of QAtlas on YouTube.<br>- Integration of Mistral models announced Dec 2023/Jan 2024.<br>- Active social media presence of founders.<br>- Participation in AI‑agent community discussions (e.g., Context Engineering blog). |

Overall, Quantalogic occupies a niche as a **European‑born, sovereign‑by‑design agent platform** with a strong technical pedigree in agent orchestration and knowledge‑graph‑enhanced RAG.

*Sources:* Product page, competitive analysis from web search, founder blog posts, market mentions.

---

## 6. Risks & Opportunities

### Risks
1. **Execution Risk:** The company is very early (founded 2024) with a small team; delivering a differentiated enterprise platform at scale is challenging.
2. **Adoption Risk:** Enterprises may hesitate to adopt a nascent vendor over established players (even if those require data to leave Europe).
3. **Funding Sustainability:** The seed round provides runway, but future Series A will depend on demonstrable revenue and product‑market fit.
4. **Technical Complexity:** Maintaining a high‑performance hybrid Graph RAG system requires ongoing expertise in both vector databases and graph databases.
5. **Regulatory Evolution:** While sovereignty is a strength, evolving EU AI Act provisions could impose new compliance burdens.

### Opportunities
1. **Data Sovereignty Demand:** Growing EU‑based concern over data privacy and US cloud reliance creates a tailwind for sovereign AI providers.
2. **Government & Public Sector:** French administration and EU institutions are potential early adopters for secure document‑intelligence tools.
3. **Partnerships:** Collaborations with European cloud providers (OVHcloud, Scaleway, Hetzner) or AI‑hardware firms could strengthen the stack.
4. **Open‑Source Credibility:** Founders’ involvement in open‑source agent frameworks (EdgeCrab) can attract developer community and reduce vendor lock‑in perceptions.
5. **Vertical Solutions:** QAtlas can be tailored to industry‑specific knowledge graphs (e.g., legal case law, medical ontologies) to create defensible moats.

---

## 7. Conclusion

Quantalogic represents a promising early‑stage effort to build a **European sovereign AI agent platform** that addresses key enterprise concerns: data privacy, explainability, and reduced hallucination through knowledge‑graph‑enhanced RAG. The announced $13.5 M seed round provides financial backing to pursue product development and market entry. While the company faces typical early‑stage risks, its founding team’s technical background, clear differentiation on sovereignty, and timing with rising EU demand for trustworthy AI position it well for growth—provided it can convert technology demonstrations into paying customers and navigate the competitive landscape of both US incumbents and emerging European rivals.

---

## 8. Sources

1. INSEE Sirene extract – Annuaire des Entreprises (data.gouv.fr): https://annuaire-entreprises.data.gouv.fr/entreprise/quantalogic-930719851  
2. LinkedIn Company Page: https://fr.linkedin.com/company/quantalogic  
3. Founder Raphael Mansuy X/Twitter post announcing seed funding: https://x.com/raphaelmansuy/highlights  
4. Playground Global portfolio references (CB Insights, Vestbee, etc.)  
5. Quantalogic product landing page: https://www.quantalogic.app/  
6. YouTube demo – “Introducing Knowledge Graph RAG”: https://www.youtube.com/watch?v=nvEsw3xkrGw  
7. LinkedIn post – Mistral models on QuantaLogic (Dec 2023): https://www.linkedin.com/posts/raphaelmansuy_mistral-ais-new-models-are-already-available-activity-7402383437212577793-4P_C  
8. Tracxn company profile: https://tracxn.com/d/companies/quantalogic/__PBbvh7zWyCEW1GY36wwVJj4w7GBfbldFSEA1X7JEdJU  
9. Pappers company extract: https://www.pappers.fr/entreprise/quantalogic-930719851  
10. Various web searches on Quantalogic, QAtlas, Graph RAG, and sovereign AI (performed 21 Apr 2026).  

*All sources accessed between 15‑21 April 2026.*

---  
*End of Document*  