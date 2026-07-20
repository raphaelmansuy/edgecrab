# 📊 Rapport Boursier — 4 Titres (Yahoo Finance)

**Horodatage (UTC) :** 2026-07-17 20:00:01 UTC
**Sources :** Yahoo Finance API (`v8/finance/chart`) + pages de cotation (`finance.yahoo.com/quote`)
**Symboles :** MSFT, AAPL (USD) · SAF.PA, AI.PA (EUR)

> Prix, variation du jour, plage du jour, volume et variation % issus du flux temps réel (`v8/finance/chart`).
> Capitalisation, P/E, EPS, Beta, rendement/dividende, cible 1 an, ex-dividende et date de résultats issus des pages de cotation Yahoo.
> *SAF.PA : la capitalisation intraday n'était pas publiée sur la page Yahoo au moment de l'extraction (marquée N/A).*

---

## 📋 Tableau Récapitulatif

| Indicateur | MSFT (USD) | AAPL (USD) | SAF.PA (EUR) | AI.PA (EUR) |
|---|---|---|---|---|
| **Prix actuel** | $393.82 | $333.74 | €329.50 | €176.46 |
| **Variation (abs)** | −$7.28 | +$0.48 | +€0.40 | −€0.62 |
| **Variation (%)** | −1.82 % | +0.14 % | +0.12 % | −0.35 % |
| **Clôture préc.** | $401.10 | $333.26 | €329.10 | €177.08 |
| **Capitalisation** | $2.925 T | $4.902 T | N/A* | €112.205 B |
| **P/E (TTM)** | 23.47 | 40.50 | 19.19 | 31.97 |
| **EPS (TTM)** | $16.78 | $8.24 | €17.17 | €5.52 |
| **Plage 52 sem.** | $349.20 – $555.45 | $201.50 – $334.99 | €262.60 – €360.80 | €140.78 – €182.26 |
| **Plage du jour** | $389.39 – $398.39 | $329.00 – $334.98 | €323.80 – €330.50 | €175.56 – €177.40 |
| **Rdt dividende** | 0.92 % | 0.32 % | 1.02 % | 1.91 % |
| **Div. à terme** | $3.64 | $1.08 | €3.35 | €3.36 |
| **Beta (5Y)** | 1.13 | 1.10 | 0.96 | 0.65 |
| **Volume** | 33,049,842 | 63,407,059 | 602,367 | 724,764 |
| **Vol. moyen** | 39,362,733 | 54,830,800 | 725,701 | 943,613 |
| **Cible 1 an** | $558.21 | $318.25 | €353.34 | €190.06 |
| **Ex-dividende** | 2026-08-20 | 2026-05-11 | 2026-05-26 | 2026-05-18 |
| **Date résultats** | 2026-07-29 | 2026-07-30 | — | 2026-07-28 |

---

## 🔍 Analyse Comparative

### 🏆 Capitalisation la plus élevée
- **AAPL — $4.902 T** (très largement en tête, ~1.68× MSFT à $2.925 T).
- AI.PA (€112.2 B) est le plus petit des 4 ; SAF.PA non disponible (N/A).

### 📈 P/E (TTM) — le plus élevé / le plus bas
- **Plus élevé : AAPL — 40.50** (valorisation la plus généreuse).
- **Plus bas : SAF.PA — 19.19** (le moins cher en multiple de bénéfices).
- Ordre : AAPL (40.50) > AI.PA (31.97) > MSFT (23.47) > SAF.PA (19.19).

### 💰 Rendement de dividende le plus élevé
- **AI.PA — 1.91 %** (L'Air Liquide, le meilleur rendement).
- Classement : AI.PA (1.91 %) > SAF.PA (1.02 %) > MSFT (0.92 %) > AAPL (0.32 %).

### 📉 Beta (5Y) — le plus bas / le plus élevé
- **Plus élevé : MSFT — 1.13** (le plus sensible au marché).
- **Plus bas : AI.PA — 0.65** (le plus défensif / faible corrélation).
- Ordre décroissant : MSFT (1.13) > AAPL (1.10) > SAF.PA (0.96) > AI.PA (0.65).

### 📊 Performance quotidienne (variation %)
- **MSFT : −1.82 %** (plus forte baisse, $-7.28)
- **AI.PA : −0.35 %**
- **SAF.PA : +0.12 %**
- **AAPL : +0.14 %** (seule hausse notable avec SAF.PA ; +$0.48)
- 🟢 En hausse aujourd'hui : AAPL, SAF.PA · 🔴 En baisse : MSFT, AI.PA.

---

## 🌍 Heures d'Ouverture des Marchés

| Marché | Place | Ouvre (heure locale) | UTC | Ferme (heure locale) | UTC |
|---|---|---|---|---|---|
| **NASDAQ** | MSFT, AAPL | 09:30 EST | 14:30 UTC | 16:00 EST | 21:00 UTC |
| **Euronext Paris** | SAF.PA, AI.PA | 09:00 CET | 08:00 UTC | 17:30 CET | 16:30 UTC |

- ⏰ NASDAQ : lundi–vendredi, fermé les jours fériés US. Décalage été (EDT) : −4h UTC.
- ⏰ Euronext Paris : lundi–vendredi, fermé les jours fériés FR/EU. En été (CEST) : +2h UTC.
- 📌 Au moment de l'extraction (2026-07-17 20:00 UTC) : **NASDAQ est fermé** (après 21:00 UTC) et **Euronext Paris est fermé** (après 16:30 UTC) — les cours affichés sont donc ceux de la clôture / dernière séance.

---

*Rapport généré automatiquement via le profil homelab EdgeCrab. Données non audité à des fins de trading ; vérifier sur Yahoo Finance avant toute décision.*
