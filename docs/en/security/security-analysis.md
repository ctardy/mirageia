# Security Analysis -- CISO Perspective

> This document is intended for Chief Information Security Officers (CISOs) evaluating the risks associated with the use of LLM-based AI assistants (Claude Code, GitHub Copilot, ChatGPT, etc.) and the protections that MirageIA can provide.

---

## Table of Contents

1. [Reference Scenario](#1-reference-scenario)
2. [What Transits to the LLM API](#2-what-transits-to-the-llm-api)
3. [Risk Mapping for the CISO](#3-risk-mapping-for-the-ciso)
4. [Protections Provided by MirageIA](#4-protections-provided-by-mirageia)
5. [Risks x Protections Matrix](#5-risks-x-protections-matrix)
6. [Limitations and Uncovered Scope](#6-limitations-and-uncovered-scope)
7. [Regulatory Compliance](#7-regulatory-compliance)
8. [Recommendations for the CISO](#8-recommendations-for-the-ciso)

---

## 1. Reference Scenario

Let's take the most common concrete case: a developer installs **Claude Code** (Anthropic's CLI) on their workstation to assist with software development.

### 1.1 Installation and Local Access

```
Developer's workstation
|
+-- /c/dev/projects/mon-projet/        <-- working directory
|   +-- src/                           <-- source code
|   +-- .env                           <-- environment variables (secrets!)
|   +-- config/database.yml            <-- database credentials
|   +-- docker-compose.yml             <-- infrastructure
|   +-- tests/fixtures/users.json      <-- test data (potential PII)
|   +-- ...
|
+-- Claude Code installed (CLI)
|   +-- Read access to ALL project files
|   +-- Write access (with user confirmation)
|   +-- Terminal access (command execution)
|   +-- Git access (history, diff, blame)
|
+-- Anthropic API key configured
    +-- ANTHROPIC_API_KEY=sk-ant-...
```

**Critical point**: as soon as it is installed, Claude Code potentially has access to **all project files**, including those containing sensitive data.

### 1.2 How Claude Code Interacts with the Developer

```
Developer                            Claude Code (local)
    |                                      |
    |-- "Refactor the login function" ---->|
    |                                      |
    |                                      |-- Reads src/auth/login.ts
    |                                      |-- Reads src/auth/middleware.ts
    |                                      |-- Reads .env (to understand context)
    |                                      |-- Reads config/database.yml
    |                                      |-- Runs git log (recent history)
    |                                      |
    |                                      |-- Builds a prompt containing:
    |                                      |   - The content of files read
    |                                      |   - The project context
    |                                      |   - The user's instruction
    |                                      |   - Command output results
    |                                      |
    |                                      |==================================|
    |                                      |  SENT TO THE ANTHROPIC API       |
    |                                      |  (via HTTPS to the cloud)        |
    |                                      |==================================|
    |                                      |
```

---

## 2. What Transits to the LLM API

### 2.1 Anatomy of an API Call

Each interaction with Claude Code generates an HTTP POST call to `api.anthropic.com/v1/messages`. Here is what is sent:

```json
{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 8096,
  "system": "You are Claude Code, a development assistant...",
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "Here is the content of src/auth/login.ts:\n\n
                   import { db } from '../config';\n
                   const DB_PASSWORD = 'P@ssw0rd_Pr0d!';\n
                   const API_SECRET = 'sk-secret-xyz123';\n
                   ...\n\n
                   Refactor this function."
        }
      ]
    }
  ]
}
```

### 2.2 Types of Data in Transit

| Data Type | How It Ends Up in the API | Frequency | Risk |
|---|---|---|---|
| **Source file contents** | Claude Code reads files and includes them in the prompt | Very frequent | Medium |
| **Configuration files** | `.env`, `database.yml`, `docker-compose.yml` | Frequent | **Critical** |
| **Hardcoded credentials** | Passwords, API keys, tokens in the code | Frequent | **Critical** |
| **Test data / fixtures** | `users.json`, SQL dumps, CSV with real data | Occasional | **High** |
| **Git history** | Commits, diffs, blames (may contain PII) | Frequent | Medium |
| **Command output** | Output from `npm test`, `docker ps`, error logs | Frequent | Medium |
| **People's names** | In code, comments, git blame, data | Frequent | High |
| **Email addresses** | In code, configs, test data | Frequent | High |
| **IP addresses / server names** | In configs, logs, deployment scripts | Frequent | **High** |
| **Internal URLs** | Intranet, internal APIs, dashboards | Frequent | High |
| **Database schemas** | Migrations, ORM models, SQL queries | Occasional | Medium |
| **SSH keys / certificates** | If present in the project directory | Rare | **Critical** |
| **Images / screenshots** | Error screenshots, UI mockups | Occasional | Medium |
| **PDF / Office documents** | Internal documentation, specs | Rare | High |
| **Binaries** | Not sent directly (Claude Code does not read binaries) | Never | -- |

### 2.3 What Claude Code Actually Sends

#### A. The System Prompt

The system prompt contains global behavior instructions, the contents of the project's `CLAUDE.md` files (which may contain information about internal architecture), and user instructions.

```
+-- System prompt -------------------------------------------------------+
| Claude Code behavior instructions                                       |
| + Contents of CLAUDE.md (project conventions)                           |
| + Environment context (OS, shell, git status)                           |
|                                                                         |
| WARNING: May contain server names, internal conventions,                |
|   database names, architecture patterns                                 |
+-------------------------------------------------------------------------+
```

#### B. User Messages (content)

Each message contains the user's instruction **plus** the contents of the files that Claude Code decided to read in order to respond:

```
+-- User message --------------------------------------------------------+
|                                                                         |
| "Refactor the authentication function"                                  |
|                                                                         |
| + Results of tools used by Claude Code:                                 |
|                                                                         |
|   +-- Read(src/auth/login.ts) ----------------------------------------+ |
|   | const DB_HOST = '192.168.1.50';                                    | |
|   | const DB_PASSWORD = 'P@ssw0rd_Pr0d!';                              | |
|   | // Author: jean.dupont@entreprise.fr                                | |
|   | function login(email: string, password: string) { ... }             | |
|   +--------------------------------------------------------------------+ |
|                                                                         |
|   +-- Read(.env) -----------------------------------------------------+ |
|   | DATABASE_URL=postgres://admin:s3cret@db.internal:5432              | |
|   | STRIPE_SECRET_KEY=sk_live_4eC39HqLyjWDarjtT1zdp7dc                | |
|   | AWS_ACCESS_KEY_ID=AKIA...                                          | |
|   +--------------------------------------------------------------------+ |
|                                                                         |
|   +-- Bash(git log --oneline -5) -------------------------------------+ |
|   | a1b2c3d fix: login Pierre Martin (pierre@corp.fr)                  | |
|   | d4e5f6g feat: add endpoint /api/users                              | |
|   +--------------------------------------------------------------------+ |
|                                                                         |
+-------------------------------------------------------------------------+
```

**All of this transits via HTTPS to Anthropic's servers.**

#### C. Conversation History

The full conversation is re-sent with each API call. If the developer has 20 exchanges in a session, the 20th call contains the previous 19 exchanges plus the new message. The volume of transmitted data **grows over the course of the conversation**.

```
Call 1:  system + message_1                          ->  ~5 KB
Call 2:  system + message_1 + response_1 + message_2 ->  ~15 KB
Call 5:  system + full history + message_5            ->  ~80 KB
Call 20: system + full history + message_20           ->  ~500 KB+

Each call contains EVERYTHING that was previously exchanged,
including files read and secrets exposed in prior messages.
```

#### D. Images and Screenshots

Claude Code supports sending images (error screenshots, UI mockups) encoded in base64:

```json
{
  "type": "image",
  "source": {
    "type": "base64",
    "media_type": "image/png",
    "data": "/9j/4AAQSkZJRgABAQ..."
  }
}
```

Images may contain visible PII (names, emails, addresses in application screenshots).

#### E. What Does NOT Transit

| Data | Transmitted? | Reason |
|---|---|---|
| Binary files (`.exe`, `.dll`, `.so`) | No | Claude Code does not read binaries |
| Files outside the project | No (unless explicitly requested) | Scope limited to the working directory |
| Keychain / password manager | No | No access |
| System files (`/etc/shadow`, registry) | No | No access (unless running as root) |
| Local network traffic | No | No network capture |

### 2.4 Anthropic's Retention Policy

According to Anthropic's documentation (subject to change -- policies evolve):

| Aspect | API Policy |
|---|---|
| Training on API data | No (API data is not used for training) |
| Request retention | 30 days (for abuse/security), then deleted |
| Server location | United States (primarily) |
| Encryption in transit | TLS 1.2+ |
| Encryption at rest | Yes (cloud infrastructure) |
| Access by Anthropic employees | Limited, for security/abuse investigation |

**CISO note**: even though Anthropic does not use API data for training, the data **transits and is temporarily stored** on servers in the United States. For an organization subject to GDPR, this constitutes a data transfer outside the EU.

---

## 3. Risk Mapping for the CISO

### 3.1 Identified Risks

| # | Risk | Impact | Probability | Severity |
|---|---|---|---|---|
| R1 | **Credential leakage** (API keys, passwords, tokens) in prompts | Critical | High | Critical |
| R2 | **PII leakage** (names, emails, phone numbers of clients/employees) | High | High | Critical |
| R3 | **Internal architecture exposure** (IPs, server names, DB schemas) | High | High | High |
| R4 | **Intellectual property leakage** (algorithms, business logic) | High | Medium | High |
| R5 | **GDPR non-compliance** (transfer of personal data outside the EU) | High | High | High |
| R6 | **Production data exposure** (fixtures, dumps with real data) | High | Medium | High |
| R7 | **Context accumulation** (growing history multiplies exposure) | Medium | High | Medium |
| R8 | **PII in images** (screenshots containing visible data) | Medium | Low | Medium |
| R9 | **Network topology exposure** (network configs, firewalls, VPN) | Medium | Medium | Medium |
| R10 | **Prompt injection** (a malicious file manipulates the LLM's behavior) | Medium | Low | Medium |

### 3.2 Typical CISO Expectations

A CISO evaluating the adoption of LLM tools within their organization expects:

#### Visibility and Control

- **Know what leaves**: what data exits the organization's perimeter?
- **Audit logs**: trace who sent what, when, to which provider
- **Access policy**: control which files/directories are accessible to the LLM
- **Kill switch**: ability to cut off access instantly

#### Data Protection

- **No secrets in plain text** sent externally (credentials, API keys, tokens)
- **No uncontrolled PII** (names, emails, phone numbers, addresses)
- **No exploitable infrastructure data** (IPs, server names, network schemas)
- **Data classification**: do not send data classified as "Confidential" or above

#### Compliance

- **GDPR**: no transfer of personal data outside the EU without a legal basis
- **NIS2**: proportionate security measures for essential/important entities
- **Internal policy**: compliance with the organization's security charter
- **Traceability**: ability to prove to auditors that data is protected

#### Reversibility and Sovereignty

- **No vendor lock-in**: ability to change LLM providers without loss
- **Local control**: protection mechanisms run on internal infrastructure
- **Independence**: no dependency on a third-party service for protection

---

## 4. Protections Provided by MirageIA

### 4.1 Protection by Data Type

#### A. Credentials and Secrets

| Data | Example | MirageIA Protection |
|---|---|---|
| Hardcoded password | `password = "P@ssw0rd!"` | Detection -> replacement with `password = "Tr0ub4dor!"` |
| API key | `sk-live-4eC39HqLyjW...` | Detection -> replacement with `sk-live-a1B2c3D4e5F...` (same format) |
| JWT token | `eyJhbGci...` | Detection -> replacement with a fictitious JWT |
| Connection string | `postgres://admin:s3cret@db:5432` | Detection of each sensitive component (user, password, host) |
| SSH private key | `-----BEGIN RSA PRIVATE KEY-----` | Detection of the full block -> replacement with a fictitious key |
| Environment variable | `AWS_SECRET_ACCESS_KEY=AKIA...` | Detection of `VARIABLE=value` pattern -> value pseudonymized |

#### B. Personal Data (PII)

| Data | Example in Code | MirageIA Protection |
|---|---|---|
| Person's name | `// Author: Jean Dupont` | -> `// Author: Michel Martin` |
| Email | `admin@entreprise.fr` | -> `paul@example.com` |
| Phone number | `+33 6 12 34 56 78` | -> `+33 6 98 76 54 32` |
| IP address | `192.168.1.50` | -> `10.0.42.7` |
| IBAN | `FR76 3000 6000...` | -> `FR14 2004 1010...` (valid checksum) |
| Social security number | `1 85 07 75 123 456 78` | -> `2 91 03 13 987 654 32` |
| Postal address | `12 rue de la Paix, Paris` | -> `8 avenue Victor Hugo, Lyon` |

#### C. Infrastructure Data

| Data | Example | MirageIA Protection |
|---|---|---|
| Internal server IP | `db.internal:5432` | -> `db.example.local:5432` |
| Internal domain name | `jira.corp.entreprise.fr` | -> `jira.internal.example.com` |
| Internal URL | `https://gitlab.corp/projet/repo` | -> `https://gitlab.example.com/projet/repo` |
| Server name | `srv-prod-db-01` | -> `srv-app-01` |
| File path | `/opt/entreprise/data/clients.db` | -> `/opt/app/data/database.db` |
| Network range | `10.42.0.0/16` | -> `10.0.0.0/16` |

#### D. Business Data

| Data | Example | MirageIA Protection |
|---|---|---|
| Client names in fixtures | `{"name": "Societe Durand"}` | -> `{"name": "Societe Example"}` |
| Financial amounts | `total: 1_547_892.50EUR` | Not pseudonymized (not direct PII) |
| Contract number | `CTR-2024-00547` | Contextual detection -> pseudonymized if pattern identified |

### 4.2 Conversation History Protection

MirageIA pseudonymizes **every API call**, including the accumulated history:

```
Call 1:
  User: "Refactor login.ts" + file content
  -> MirageIA pseudonymizes the file content

Call 5:
  History (calls 1-4 included) + new message
  -> Pseudonyms are CONSISTENT: "Tardy" is always "Gerard"
  -> History contains already pseudonymized versions
  -> The new message is also pseudonymized
```

### 4.3 What the API Sees vs What Is Real

```
+-- What the developer writes ------------------------------------------+
| "The server db-prod-01 (192.168.1.50) is down.                        |
|  Contact jean.dupont@entreprise.fr for the admin password              |
|  of the dashboard https://grafana.corp.entreprise.fr"                  |
+-----------------------------------------------------------------------+
                              |
                         MirageIA
                              |
                              v
+-- What the Anthropic API receives ------------------------------------+
| "The server srv-app-01 (10.0.42.7) is down.                           |
|  Contact paul.martin@example.com for the admin password                |
|  of the dashboard https://grafana.internal.example.com"                |
+-----------------------------------------------------------------------+
                              |
                        API response
                              |
                              v
+-- What the API responds ----------------------------------------------+
| "For server srv-app-01, I suggest checking the logs                    |
|  at /var/log/... Ask paul.martin@example.com to..."                    |
+-----------------------------------------------------------------------+
                              |
                         MirageIA
                              |
                              v
+-- What the developer receives ----------------------------------------+
| "For server db-prod-01, I suggest checking the logs                    |
|  at /var/log/... Ask jean.dupont@entreprise.fr to..."                  |
+-----------------------------------------------------------------------+
```

---

## 5. Risks x Protections Matrix

| Risk | Without MirageIA | With MirageIA | Protection |
|---|---|---|---|
| R1 -- Credential leakage | Exposed in plain text | Pseudonymized | Keys, tokens and passwords replaced with fictitious values in the same format |
| R2 -- PII leakage | Exposed in plain text | Pseudonymized | Names, emails, phone numbers, addresses replaced with consistent fictitious values |
| R3 -- Architecture exposure | Exposed in plain text | Pseudonymized | IPs, server names, internal URLs replaced |
| R4 -- Intellectual property leakage | Exposed in plain text | Partially protected | Business logic and algorithms are not pseudonymized (only data is) |
| R5 -- GDPR non-compliance | Transfer outside the EU | Anonymized data | PII is pseudonymized before transfer -- the sent data no longer constitutes personal data under GDPR |
| R6 -- Production data | Exposed in plain text | Pseudonymized | Fixtures and dumps containing real data are pseudonymized |
| R7 -- Context accumulation | Growing history | Pseudonymized | Each call is pseudonymized, history contains only pseudonyms |
| R8 -- PII in images | Sent as-is | Not protected | MirageIA v1 does not process images (known limitation) |
| R9 -- Network topology | Exposed | Pseudonymized | IPs, network ranges, server names replaced |
| R10 -- Prompt injection | Existing risk | Risk unchanged | MirageIA does not protect against prompt injection (out of scope) |

---

## 6. Limitations and Uncovered Scope

### 6.1 What MirageIA Does NOT Protect

| Limitation | Explanation | Possible Mitigation |
|---|---|---|
| **Intellectual property** | Algorithms, business logic, and code architecture are transmitted in plain text. MirageIA protects *data*, not *logic*. | Company policy limiting the types of projects allowed to use an LLM |
| **Images and screenshots** | MirageIA v1 analyzes text only, not images. PII visible in screenshots is not detected. | Developer awareness training, future extension with OCR |
| **Prompt injection** | If a malicious file contains instructions that manipulate the LLM, MirageIA does not detect this threat. | Complement with a prompt injection detection tool (LLM Guard, NeMo Guardrails) |
| **File metadata** | File names, paths, and timestamps are not pseudonymized if they appear in request metadata (outside textual content). | Future extension |
| **Volume and usage patterns** | The number of requests, their frequency, and their size remain visible to the provider. | VPN / network proxy if necessary |
| **Complex structured data** | A complete SQL schema or migration file can reveal the business structure even with pseudonymized data. | Company policy, file classification |

### 6.2 False Positives and False Negatives

| Case | Impact | Estimated Frequency |
|---|---|---|
| **False positive**: a variable `edison_voltage` is pseudonymized | The LLM's response may be incorrect (variable name changed) | Low (contextual detection) |
| **False negative**: a non-standard client identifier `CLI-847293` is not detected | Data leakage | Medium (non-standard formats) |

The confidence threshold is adjustable: a low threshold reduces false negatives but increases false positives.

---

## 7. Regulatory Compliance

### 7.1 GDPR (General Data Protection Regulation)

| GDPR Requirement | Without MirageIA | With MirageIA |
|---|---|---|
| **Art. 5 -- Minimization**: collect only necessary data | All project data is sent | PII is pseudonymized, only fictitious data is sent |
| **Art. 25 -- Privacy by design** | No built-in protection | Automatic protection by default, with no action required from the developer |
| **Art. 44-49 -- Transfer outside the EU** | Personal data sent to the USA | Sent data no longer constitutes personal data (pseudonymized reversibly only locally) |
| **Art. 32 -- Security of processing** | Data in plain text in API requests | AES-256-GCM encrypted mapping, pseudonymized data |
| **Art. 30 -- Records of processing activities** | The MirageIA dashboard can serve as a trace of processed data | Logs of detected PII types (without original values) |

### 7.2 NIS2 (Network and Information Security Directive)

| NIS2 Requirement | MirageIA Contribution |
|---|---|
| Supply chain risk management | Reduces the risk of data leakage through third-party providers (LLM providers) |
| Technical security measures | Encrypted mapping, automatic pseudonymization |
| Incident notification | The dashboard can detect abnormal patterns (unusual PII volume) |

### 7.3 SOC 2 / ISO 27001

MirageIA contributes to the following controls:
- **Data access control**: sensitive data does not leave the perimeter
- **Encryption**: AES-256-GCM for the in-memory mapping
- **Logging**: audit logs of detections (without original data)
- **Third-party management**: reduction of risk associated with AI service providers

---

## 8. Recommendations for the CISO

### 8.1 Recommended Deployment

```
+-- Organization's perimeter -------------------------------------------+
|                                                                        |
|  Developer workstation                                                 |
|  +------------------------------------------------------------------+  |
|  |                                                                  |  |
|  |  Application (Claude Code)                                       |  |
|  |       |                                                          |  |
|  |       v                                                          |  |
|  |  +----------+                                                    |  |
|  |  | MirageIA | <-- runs locally, no data leaves                   |  |
|  |  |          |   without pseudonymization                         |  |
|  |  +----------+                                                    |  |
|  |       |                                                          |  |
|  +-------+----------------------------------------------------------+  |
|          | pseudonymized data only                                     |
|          | (HTTPS)                                                     |
+----------+-------------------------------------------------------------+
           |
           v
    +--------------+
    | Anthropic API |  <-- only sees fictitious data
    | (US cloud)    |
    +--------------+
```

### 8.2 Recommended Complementary Measures

| Measure | Objective | Priority |
|---|---|---|
| **Strict `.gitignore`** | Prevent sensitive files from being in the repo (`.env`, keys) | High |
| **`.claudeignore`** | Exclude files from being read by Claude Code | High |
| **Developer training** | Raise awareness about data leakage risks via LLMs | High |
| **Project classification** | Prohibit LLM usage on projects classified "Confidential" and above | High |
| **Network monitoring** | Monitor the volume of data sent to LLM APIs | Medium |
| **API key rotation** | Limit the impact of an API key exposed in a prompt | Medium |
| **Fixture review** | Replace real data in test files with fictitious data | Medium |
| **Periodic audit** | Verify that MirageIA correctly detects PII in the business context | Medium |

### 8.3 Monitoring Indicators (KPIs)

| KPI | Description | Target |
|---|---|---|
| PII coverage rate | % of PII detected vs actual PII (measured by audit) | > 95% |
| False positive rate | % of incorrect detections | < 5% |
| Number of intercepted credentials | Credentials that would have been sent without MirageIA | Monthly tracking |
| Added latency | Impact on developer productivity | < 100ms |
| Adoption | % of developers using MirageIA | 100% (mandatory) |

### 8.4 Talking Points for Management

> **Without MirageIA**: every developer using Claude Code, Copilot, or ChatGPT potentially sends passwords, customer data, production server IP addresses, and API keys to cloud servers in the United States. The organization has no visibility or control over these leaks.
>
> **With MirageIA**: a transparent local proxy automatically pseudonymizes all sensitive data before it leaves the workstation. Developers retain the productivity offered by LLMs, while IT security retains control over the data. All in a single binary, with no cloud dependency, no recurring cost, and GDPR-compliant.
