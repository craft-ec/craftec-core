# Craftec Vision — Complete Architecture

## What Craftec Is

Craftec is dumb data infrastructure. Store bytes. Retrieve bytes. Keep them alive. That's the entire product.

No opinions about consensus. No opinions about privacy. No opinions about finance. No opinions about anything.

The less the infrastructure assumes, the more applications it supports. TCP/IP won because it just moves packets. Craftec is TCP/IP for storage.

---

## The Stack

```
┌─────────────────────────────────────────────┐
│               Applications                  │
│        Any app that reads/writes data       │
├─────────────────────────────────────────────┤
│               CraftVFS                      │
│     Distributed filesystem (ZFS-like)       │
│  Inode table + directory entries on CraftSQL│
├─────────────────────────────────────────────┤
│               CraftSQL                      │
│     SQLite VFS backed by CraftOBJ           │
│     Standard SQL. Portable. Pluggable.      │
├─────────────────────────────────────────────┤
│               CraftOBJ                      │
│     Distributed object storage              │
│     RLNC erasure + P2P + self-healing       │
├─────────────────────────────────────────────┤
│            craftec-core                      │
│        Shared infrastructure                │
│  Identity, crypto, erasure, P2P transport   │
│  DHT + PEX + wire protocol + NAT relay      │
└─────────────────────────────────────────────┘

Products:
  CraftOBJ — distributed object storage
  CraftSQL — SQLite VFS on CraftOBJ
  CraftVFS — distributed filesystem on CraftSQL
  CraftNET — decentralized VPN (formerly CraftNet)
  CraftSEC — on-demand MPC functions (transaction attestation)
  CraftCPU — persistent CPU agents (gateway, bots, services)
  CraftGPU — persistent GPU agents (inference, ZK, media)
```

Each product is independently useful. All share craftec-core for networking, identity, and crypto.

The infrastructure never executes content — it only stores and retrieves.

CraftSEC is the trust layer. Every financial write goes through CraftSEC's MPC threshold signing — the program validates the transaction and multiple nodes co-sign. Without CraftSEC, users could write anything to their own chains. With CraftSEC, every entry is program-attested and cryptographically uncontestable.

---

## craftec-core — Shared Infrastructure

P2P connectivity and crypto primitives shared by all products. Same model as BitTorrent.

- **DHT (Kademlia)**: peer routing, key-value records, mutable signed records
- **PEX (Peer Exchange)**: peers share known peer lists
- **P2P request/response**: direct node-to-node communication
- **NAT traversal**: relay nodes + hole punching via DCUtR
- **Identity**: DIDs, signing keys, capability announcements
- **Erasure coding**: RLNC over GF(2^8), homomorphic hashing
- Zero gossipsub — all communication is pull-based or direct
- Proven at 10M+ concurrent nodes (BitTorrent)

---

## CraftOBJ — Distributed Object Storage

Store and retrieve content-addressed objects across a volunteer P2P network.

### Two-Tier Model

**Free tier (BitTorrent-style altruism):**
- You store pieces for others, others store pieces for you
- Tit-for-tat reciprocity, no tokens, no financial overhead
- Best-effort persistence — data survives if enough nodes stay
- Self-healing via HealthScan detects and repairs missing pieces

**Paid tier (guaranteed erasure minimum):**
- Pay to ensure N pieces always exist
- PDP (Proof of Data Possession) verifies nodes hold pieces
- Solana settlement enforces contracts
- For data that must survive regardless of altruism

### Storage Strategy

- **Small objects (< 1MB)**: replicate N copies (simple, efficient for SQLite pages and financial data)
- **Large objects (> 1MB)**: RLNC erasure coding with 256KB pieces (storage efficient for files, video)

RLNC erasure coding only makes sense when K (pieces needed to reconstruct) is high. A 4KB SQLite page can't be meaningfully erasure coded — just replicate it.

### The Core Problem: Churn

Everything else — consensus, ordering, verification, integrity — is solved by the architecture. The one real problem: if pieces disappear faster than healing can replace them, data dies.

```
Data alive: healing rate > churn rate
Data dead:  churn rate > healing rate
```

RLNC's advantage: any surviving node can generate a new unique piece without needing the original data. This dramatically increases healing rate compared to traditional erasure coding.

### Scale Comparison (Bitcoin-equivalent transaction volume)

```
Bitcoin:  600GB × 20,000 nodes = 12,000 TB network-wide (every node stores everything)
Craftec: 100GB × 30 replicas = 3 TB total (distributed across all nodes)
         With 20,000 nodes: each stores ~150MB
         That's less than a phone app. Anyone can participate.
```

---

## Layer 3: CraftSQL — SQLite VFS on CraftOBJ

Not a new database engine. A **SQLite Virtual File System** that maps page I/O to CraftOBJ.

### Why SQLite VFS

- Battle-tested over 20+ years, billions of deployments
- Full SQL, query planner, indexes, transactions — all free
- Massive ecosystem (every language has bindings)
- Public domain — no licensing issues
- Precedent exists: sql.js, cr-sqlite, Litestream, LiteFS

### How It Works

```
SQLite internally:
  xRead(page 42)  → VFS fetches CID from CraftOBJ
  xWrite(page 42) → VFS encodes new CID, stores in CraftOBJ
  xSync()         → VFS updates page table CID, publishes to DHT
```

The application writes SQL. SQLite handles query planning, indexing, transactions. The VFS handles CID mapping. CraftOBJ handles storage.

### Page Table

A page table is just a mapping: page number → CID.

| Database size | Pages | Page table size |
|---|---|---|
| 1 MB | 256 | ~10 KB |
| 10 MB | 2,560 | ~100 KB |
| 100 MB | 25,600 | ~1 MB |

For personal data use cases, the page table is tiny — fits in a single CID fetch.

### Tracking the Page Table

One DHT signed record per database. That's the only mutable thing.

```
DID:alice:mydb → CID_of_page_table (latest version)

DHT signed record (mutable, one per database)
└→ Page table CID (immutable)
   ├→ Page 0 CID (immutable)
   ├→ Page 1 CID (immutable)
   └→ ...
```

Every write creates new immutable CIDs and updates one mutable pointer. Old CIDs persist in the network — that's free version history. A snapshot is just saving the old page table CID.

### Latency — The Real Challenge

SQLite assumes disk (microsecond reads). CraftOBJ responds in milliseconds to seconds.

Solution: local-first, sync later.

```
App → SQLite (local, fast, microseconds)
      ↓ background sync
      CraftOBJ (network, slow, durability + sharing)
```

- Write to local SQLite immediately
- Background sync to CraftOBJ
- Aggressive local caching of pages
- Merkle diffs to keep cache fresh
- User never waits for the network

### Pluggable PageStore

CraftSQL's real value: the PageStore interface is swappable.

```rust
trait PageStore {
    fn get(&self, cid: &Cid) -> Result<Page>;
    fn put(&self, page: &Page) -> Result<Cid>;
    fn update_root(&self, new_root: Cid) -> Result<()>;
}
```

- CraftOBJ implements it (distributed P2P)
- Local disk implements it (offline/single-machine)
- S3/R2 implements it (traditional infra)
- Anyone can implement their own

Develop and test against local PageStore, plug in CraftOBJ when ready.

### CraftVFS uses a Database interface (not PageStore)

```rust
trait Database {
    fn query(&self, sql: &str, params: &[Value]) -> Result<Rows>;
    fn execute(&self, sql: &str, params: &[Value]) -> Result<()>;
}
```

CraftSQL, SQLite, PostgreSQL, DuckDB — any can implement it. Two plugin points at two different abstraction levels.

---

## Layer 4: CraftVFS — Distributed Filesystem

A filesystem built as CraftSQL tables. Uses inode-style design for O(1) renames.

### Schema

```sql
CREATE TABLE inodes (
    id INTEGER PRIMARY KEY,
    cid TEXT,
    type TEXT,  -- 'file' | 'dir'
    size INTEGER,
    owner TEXT  -- DID
);

CREATE TABLE dirents (
    parent_id INTEGER,
    name TEXT,
    inode_id INTEGER,
    PRIMARY KEY (parent_id, name)
);
```

Rename a directory = update one row in dirents. O(1).

### ZFS Features — For Free

| Feature | How |
|---|---|
| Copy-on-write | Every mutation creates new CID pages (inherent) |
| Snapshots | Bookmark current root page CID. Instant, zero cost. |
| Checksums | CID IS the checksum |
| Self-healing | CraftOBJ HealthScan + RLNC repair |
| RAID-Z | RLNC erasure across network nodes |
| Deduplication | Same content = same CID. Automatic. |
| Clones | New entry pointing to same CID. Zero cost. |

### Beyond ZFS

Survives entire machine loss, geographic redundancy, shareable via DID, unlimited snapshots, global namespace.

---

## Single-Owner Writes — The Core Insight

Every user owns their own database. Only the owner writes to it. This eliminates the need for consensus.

### Why Consensus Exists

Blockchain complexity comes from ONE decision: "multiple untrusted parties write to shared state."

That requires consensus protocols, global ordering, state machine replication, gas metering, VM sandboxing, fork choice rules, finality gadgets, sybil resistance, slashing.

Craftec removes that decision. Single owner writes. The entire tower of complexity vanishes.

### Everything Decomposes to Single-Owner

```
Messaging: each person writes to their own DB, app reads from both
Social media: your posts live in your DB, feed = reading followed users
Marketplace: your listings in your DB, search = query across sellers
Email: already works this way — outbox is yours, inbox is theirs
```

Even "multi-writer" problems decompose:

```
Inventory: seller owns inventory DB, seller decides who gets last item
Bank transfer: Alice writes "send $100" (signed), Bob reads and verifies
Game state: players write inputs, host writes resolved state
Stock exchange: each trader writes orders, exchange is single-writer matcher
```

Every "consensus" problem decomposes into individual writes + a coordinator that is itself a single writer.

### Financial Chain Security

Single-owner writes make forking structurally impossible. CraftSEC MPC attestation makes fraud cryptographically impossible.

```
One DID → one DHT entry → one page table CID → one database → one transaction history
```

Alice can't fork because:
- She can't create two different versions of the same CID (content-addressed)
- She can't rewrite history (hash-linked chain)
- Everyone reads the same DHT key (public)
- She can't write invalid entries (CraftSEC threshold nodes must co-sign)

Alice can't lie about her balance because:
- Every transaction is attested by the program's MPC signature
- The program reads her actual balance before signing
- Invalid balance = program refuses to sign = no valid entry

SQL queries verify everything:

```sql
-- Check balance
SELECT SUM(CASE
    WHEN recipient = 'alice' THEN amount
    WHEN sender = 'alice' THEN -amount
END) as balance
FROM transactions
WHERE sender = 'alice' OR recipient = 'alice';

-- Verify program attestation
SELECT * FROM transactions
WHERE program_sig IS NULL
   OR program_cid NOT IN (SELECT cid FROM trusted_programs);
-- Should return zero rows. Any result = invalid entry.
```

No blocks. No mining. No staking. No consensus. Just cryptography, MPC attestation, and a public log.

---

## Privacy — Encryption at the VFS Boundary

Privacy is a flag, not a feature.

```
// Public database
let db = sqlite3_open("craftobj://alice/photos");

// Private database
let db = sqlite3_open("craftobj://alice/photos?encrypt=true");
```

### Three Levels

```
Level 1 — Public:     anyone reads, anyone verifies
Level 2 — Encrypted:  nobody reads, nobody verifies
Level 3 — ZK private: nobody reads, anyone can verify (app-level ZK proofs)
```

### How Encryption Works

```
Write: SQLite page → encrypt with user's key → CID of ciphertext → CraftOBJ
Read:  Fetch CID → get ciphertext → decrypt → SQLite page
```

Nodes store encrypted blobs. They never see the data. The VFS handles this transparently.

### ZK for Private Verifiable Chains

ZK is application-level logic, not infrastructure. The VFS encrypts. The application generates proofs and stores them as regular data in a public table.

```
Alice's DB (encrypted): transactions table
Alice's DB (public):    proofs table

Bob verifying: fetch proof → verify math → accept payment
               Never sees Alice's balance or history.
```

Every "private blockchain" (Zcash, Monero, Aztec, Railgun) embeds privacy into the protocol. Craftec keeps it in the application layer — simpler, flexible, each app picks its own ZK scheme.

### Encryption vs Dedup Tradeoff

Encrypted data can't deduplicate across users (different keys = different CIDs). Accept this: dedup works within a user's namespace only. Public data deduplicates. Private data doesn't. User decides.

---

## CraftSEC — On-Demand MPC Functions (The Trust Layer)

Users own their chains and can write anything. Without attestation, a user could post a fake transaction claiming they have funds they don't.

Optimistic verification ("someone will check later") fails in practice — nobody has incentive to run verification nodes for millions of small transactions. Fraud goes unchecked.

CraftSEC solves this with MPC threshold signatures. Every financial write must be validated and co-signed by a program running on threshold nodes. No valid signature = no valid transaction. Not optimistic. Deterministic.

### How It Works

```
Alice wants to send $50 to Bob:

1. Alice submits request to CraftSEC:
   {fn: Qm_transfer_abc, args: {to: bob, amount: 50}}

2. Request goes to 3 threshold nodes (randomly selected)

3. Each node independently:
   - Loads function from CID (cached after first load)
   - Reads Alice's balance from CraftSQL
   - Validates balance >= 50
   - Computes output transaction
   - Produces signature SHARE
   Time: <50ms each (it's just SQL queries)

4. Alice collects 2-of-3 shares → combines → full signature

5. Entry written to Alice's chain:
   {
     sender: alice,
     recipient: bob,
     amount: 50,
     user_sig: <Alice's signature>,          ← proves intent
     program_cid: Qm_transfer_abc,           ← which code ran
     program_sig: <MPC threshold signature>  ← proves code validated it
   }

Total added latency: ~100ms
Total added complexity for user: zero (SDK handles it)
```

### Why MPC Over Optimistic

```
Optimistic:
  "Trust but verify"
  → Assumes someone will check
  → In practice, nobody checks small transactions
  → Fraud goes undetected until damage is done
  → Challenge periods add latency (7 days on L1 rollups)

MPC:
  "Can't write without proof"
  → Valid signature = valid transaction. Period.
  → No challenge window. No watchers needed.
  → Fraud is structurally impossible, not economically unlikely.
  → Not a spectrum. A binary.
```

### Program Derived Keys (PDK)

Programs own keys. No human has access. Same concept as Solana PDAs, but enforced by threshold cryptography instead of a VM runtime.

```
Deploy a program:
1. Developer publishes code → gets CID (Qm_transfer_abc)
2. Network generates a keypair FOR that CID:
   - Distributed Key Generation (DKG) across threshold nodes
   - Each node gets a SHARD of the private key
   - Full private key never exists anywhere
   - Public key = the program's identity
3. Program is now deployed:
   Program CID: Qm_transfer_abc
   Program key:  craft1_xyz... (public, verifiable)
   Key shards:  held by N threshold nodes
```

```
Solana PDA:
  Key derived from: program_id + seeds
  Signing enforced by: Solana runtime (VM)
  Trust assumption: validators run honest VM

Craftec PDK:
  Key derived from: program CID + seeds
  Signing enforced by: threshold nodes (DKG)
  Trust assumption: majority of threshold nodes honest

Same concept. The PROGRAM owns the key. No human has access.
Only correct execution produces a valid signature.
```

### Programs Can Hold Assets

A program key can own balances — just like a Solana PDA holds tokens:

```
Program: Qm_swap_program
Program key: craft1_swap...

This key can OWN assets:
  In CraftSQL: balance entries where owner = craft1_swap...
  On-chain escrows: USDC held by program's derived key

Only the swap program (verified by CID) can move these funds.
No human can access them.

→ This IS a smart contract wallet
→ This IS a DEX liquidity pool
→ This IS a DAO treasury
→ This IS an escrow
```

### Multiple Keys Per Program

Programs can derive multiple keys for different purposes (like Solana PDAs with different seeds):

```
transfer_program_key     = DKG(Qm_transfer + "main")
escrow_key_alice_bob     = DKG(Qm_transfer + "escrow:alice:bob")
treasury_key             = DKG(Qm_transfer + "treasury")

Each key is independently threshold-managed.
Each requires correct program execution to sign.
Program logic decides WHICH key signs WHEN.
```

### Key Rotation

Proactive Secret Sharing allows key shard rotation without changing the public key:

```
Periodically:
- Re-share key shards to new set of nodes
- Old shards become useless
- Public key stays the same
- Even if attacker stole old shards → can't use them

Adds time dimension to security.
Attacker must compromise threshold nodes simultaneously.
```

### Why Not Other Approaches

```
TEE (Trusted Execution Environments):
  Hardware enclaves (Intel SGX) attest code execution.
  Fast, simple, privacy built in.
  BUT: trusts hardware manufacturer, side-channel attacks, vendor lock-in.
  Against Craftec's "no trust in hardware."

Optimistic / Fraud Proofs:
  Assume valid, challenge if wrong. Cheapest and simplest.
  BUT: nobody checks, challenge periods add latency, no enforcement mechanism without bonds/slashing.

ZK Validity Proofs:
  Mathematically prove execution was correct. Strongest guarantee, instant finality.
  BUT: heavy computation for proof generation.
  Used at APPLICATION level (CloakCraft) for privacy, not at infrastructure level for every transaction.

MPC Threshold Signatures:
  ✓ No hardware trust (pure cryptography)
  ✓ No challenge period (instant finality)
  ✓ No heavy computation (SQL queries + signing)
  ✓ Proven technology (TSS libraries exist)
  ✓ Uncontestable (signature valid or not)
```

### Trust Model

```
Every financial transaction requires:
  User signature   → proves intent (Alice authorized this)
  Program MPC sig  → proves correctness (code validated it)

Both required → neither alone is sufficient
User can't bypass program → doesn't have program key
Program can't act alone → needs user's authorization
Threshold nodes can't steal → need 2-of-3 minimum
Single node can't forge → one shard is useless

Three independent guarantees. All cryptographic. None optimistic.
```

### Transaction Schema

```sql
CREATE TABLE transactions (
    seq INTEGER PRIMARY KEY,
    prev_hash TEXT NOT NULL,
    sender TEXT NOT NULL,
    recipient TEXT NOT NULL,
    amount REAL NOT NULL,
    timestamp INTEGER NOT NULL,

    -- User authorization
    user_sig TEXT NOT NULL,

    -- Program attestation (MPC)
    program_cid TEXT NOT NULL,   -- which code ran (immutable)
    program_sig TEXT NOT NULL,   -- threshold signature (unforgeable)

    -- Optional: multiple attestors for high-value tx
    attestors JSON,             -- [{cid, sig}, {cid, sig}, ...]

    hash TEXT NOT NULL           -- hash of everything above
);
```

### What CraftSEC Nodes Actually Do

```
CraftSEC node is lightweight:
- No persistent state
- No event loop
- No database

Per request:
1. Receive function call + args
2. Load function from CID (cached)
3. Read required state from CraftSQL (network call)
4. Execute function (SQL queries, ~10ms)
5. Produce signature shard
6. Return shard to caller
7. Forget everything

That's it. Stateless. On-demand. Milliseconds.
```

### Compute Taxonomy

```
CraftSEC  — On-demand MPC functions
           Stateless. Per-request. Milliseconds.
           THE trust layer for all financial writes.
           Every transaction attestation goes through CraftSEC.

CraftCPU — Persistent CPU agents
           Stateful. Always-on. Long-running.
           Gateway watchers, bots, services.

CraftGPU — Persistent GPU agents
           Stateful. Always-on. GPU hardware.
           Inference, ZK proofs, transcoding.
```

---

## Web Hosting — No Servers Needed

```
npm run build → /dist folder → upload to CraftOBJ → done
```

Every modern frontend framework produces static files. CraftOBJ serves them. Popular content caches on more nodes automatically (same as BitTorrent).

```
Vercel charges for:          Craftec equivalent:
CDN bandwidth                CraftOBJ P2P (free)
Static hosting               CraftOBJ CIDs (free)
Build minutes                Your own machine (free)
Serverless functions         Browser + CraftSQL (mostly free)
Database                     CraftSQL (user-owned)
```

SSR (server-side rendering) is mostly eliminated:
- SEO → pre-render at build time (SSG)
- Fast first paint → static HTML shell + client-side hydration
- Dynamic content → client fetches from CraftSQL directly
- Edge cases → ISR (render once, cache as CID forever)

The entire web hosting industry is: "I have files, serve them to browsers." CraftOBJ does exactly that.

---

## CraftGPU — Persistent GPU Agents (Sidecar)

GPU agents that run continuously, not batch jobs. Same agent model as CraftCPU — state in CraftSQL, code as CID, coordination via SQL tables — but on GPU hardware, serving heavy linear compute.

### Agents, Not Jobs

```
Batch mindset (traditional GPU networks):
  Submit job → split across GPUs → collect results → done
  One-shot. Request/response. GPUs idle between jobs.

Agent mindset (CraftGPU):
  GPU agent runs continuously
  Watches CraftSQL for work
  Processes requests as they arrive
  Writes results back to CraftSQL
  Never stops. Always warm. Model stays loaded.
```

### Example GPU Agents

```
Agent: inference-llama-70b
  Watches: inference_requests table
  Writes:  inference_results table
  GPU: model loaded, runs forward pass per request
  Model: weights stored as CIDs in CraftOBJ

Agent: zk-prover
  Watches: proof_requests table
  Writes:  proofs table
  GPU: generates ZK proofs continuously

Agent: transcoder
  Watches: transcode_queue table
  Writes:  transcoded_media (CIDs to CraftOBJ)
  GPU: video encoding/decoding

Agent: image-generator
  Watches: generation_requests table
  Writes:  generated_images (CIDs)
  GPU: diffusion model, always loaded
```

Each agent: state in CraftSQL, code as CID (WASM + GPU kernel), model weights as CIDs in CraftOBJ.

If host dies, new GPU node loads code CID + model CIDs + state CID → resumes.

### Coded Computation for Redundancy

RLNC coded computation — the same math that protects storage — applies to GPU agent availability and verification, not batch splitting:

```
3 GPU agents serve the same model (redundancy)
Any one can handle requests
If one dies → other 2 continue, new agent spawns
Coded computation verifies results mathematically
No need to trust any single GPU node
```

For large jobs that do need parallelism, coding still eliminates stragglers:

```
Normal: 10 GPUs process parts → wait for ALL 10 (stragglers block)
Coded:  15 GPUs process coded parts → ANY 10 finish → decode (stragglers irrelevant)
```

Research-proven for linear operations: matrix multiply, convolutions, gradient descent, ZK proof generation.

### What It Doesn't Work For

Sorting, graph traversal, general business logic — anything non-linear. These run on CraftCPU or locally.

### CraftGPU vs CraftCPU

```
              CraftCPU              CraftGPU
Hardware:     CPU node              GPU node
Workload:     logic, I/O, signing   math, inference, proofs
State:        CraftSQL              CraftSQL
Code:         WASM (CID)            WASM + GPU kernel (CID)
Coordination: SQL tables            SQL tables
Fault recovery: reload CID          reload CID + model CIDs
Model:        same                  same
```

Same agent paradigm. Different hardware. Both persistent. Both coordinate through SQL.

### Why It's Novel

Existing GPU networks (Render, Akash, io.net) are centralized marketplaces with batch job queues — AWS with extra steps and a token.

CraftGPU: persistent agents (not jobs), no platform, no token, data already on CraftOBJ (data locality), coded computation for verification + straggler elimination.

Nobody ships persistent GPU agents on a P2P network in production.

---

## CraftCPU — Persistent CPU Agents (Sidecar)

Not distributed batch compute. Persistent autonomous agents that run continuously, coordinate through SQL, and survive host failure.

### Why Agents Need Hosting

```
AI agent with MCP, trading bot, IoT edge processor, indexer,
notification service, orchestration agent — all need:
  ✓ Always running (can't close laptop)
  ✓ Lightweight CPU (logic, not math)
  ✓ Stateful (remembers context)
  ✓ Network-connected
  ✓ Autonomous
```

### Agent Model on Craftec

```
Agent state = CraftSQL database (CIDs)
Agent code  = WASM binary (CID)
Agent I/O   = reads/writes CraftOBJ

If host node dies:
  State is already in CraftOBJ
  New node loads code CID + state CID → resumes
```

### Parallelism Through Decomposition

One agent is sequential. But workflows decompose into multiple agents:

```
Agent A: watches market → writes signals to its DB
Agent B: watches news  → writes summaries to its DB
Agent C: reads A + B   → makes decisions → writes trades
Agent D: reads C       → sends notifications

Four agents. Four nodes. Parallel. Coordinated via CraftSQL.
```

No message queues. No API calls. Agents read each other's databases. SQL is the coordination layer.

### Primary Use Case: Financial Gateway Agents

CraftCPU isn't a sidecar — it's the nervous system of the financial layer. The multichain gateway (see Multichain Architecture below) is a fleet of CraftCPU agents:

```
Agent: chain-watcher-ethereum
  Loop: Read latest ETH block
        Check escrow address for incoming USDC
        INSERT INTO deposits (did, amount, chain, tx_hash, confirmations) ...
        Save checkpoint (block number) to state DB

Agent: chain-watcher-solana
  Same logic, different chain RPC

Agent: chain-watcher-base
  Same logic, different chain RPC

Agent: withdrawal-processor
  Loop: SELECT * FROM withdrawals WHERE status = 'pending'
        Generate signature share (threshold sig)
        Write signature to signatures table
        When threshold met → broadcast tx to target chain
        UPDATE withdrawals SET status = 'complete'

Agent: rebalancer
  Loop: SELECT chain, SUM(balance) FROM escrow_balances GROUP BY chain
        If imbalanced → INSERT INTO rebalance_requests ...
        Trigger CCTP transfer between chain escrows

Agent: auditor
  Loop: Compare CraftSQL balances vs actual on-chain balances
        Flag any discrepancy → INSERT INTO alerts ...
```

All coordinated through SQL. No message queues, no RPC calls, no orchestration framework. Each agent reads/writes CraftSQL tables. SQL is the coordination layer.

Trust model: 5 independent CraftCPU agents run withdrawal-processor. Each produces a signature share. 3-of-5 required to release funds. No single agent can steal. All signatures recorded in CraftSQL — public proof.

If an agent is compromised: can't steal (needs 3-of-5), can't hide (all actions in public DB), can be replaced (new agent loads same code CID).

---

## What Doesn't Exist in Craftec

| Thing | Why Not |
|---|---|
| Consensus layer | Single-owner writes eliminate the need. No multi-writer = no consensus. |
| Smart contract VM | Programs are functions (any language) validated by CraftSEC MPC. No VM, no gas, no bytecode. |
| Token/coin | Free tier is altruism. Paid tier uses USDC via multichain escrows. |
| Optimistic verification | MPC threshold signatures are deterministic. Valid signature or not. No challenge periods, no watchers. |
| CraftSYS (OS) | Running untrusted code is a fundamentally different problem than storing data. |
| Batch compute | CraftSEC is on-demand, CraftCPU and CraftGPU are persistent agents. None are batch job queues. |

---

## Compared to Everything Else

### vs Blockchain

```
Blockchain: every node processes every transaction (replication)
Craftec:    every node stores a fraction (distribution)

More nodes on blockchain = same capacity
More nodes on Craftec    = more capacity
```

```
Trust model:
  Blockchain: consensus (everyone runs the code) → VM enforces
  Craftec:    MPC threshold (3 nodes run the code) → cryptography enforces

Same guarantee: trusted code ran. 1000x less redundancy.
```

```
"On-chain" = committed + ordered + available + tamper-evident

Blockchain: hash-linked blocks, consensus-ordered
Craftec:    hash-linked CIDs, single-owner ordered, MPC-attested

Both are "on-chain." Different topology, same properties.
```

### vs Existing Systems

| System | Has | Missing |
|---|---|---|
| IPFS | Content-addressed P2P | No erasure coding, no DB, no FS |
| Filecoin | P2P + erasure coding | Economics-first, no database, no FS |
| Storj | P2P + erasure coding | Managed satellites, no DB |
| Holochain | Agent-centric + DHT | Custom framework, no SQL, no erasure coding |
| BitTorrent | P2P file sharing | No persistence guarantees, no DB |
| ZFS | COW filesystem | Local only |
| CockroachDB | Distributed SQL | Managed infra, replication not erasure |

### vs Holochain (Closest Relative)

Same philosophy (agent-centric, personal chains, no global consensus). Different execution:

| | Holochain | Craftec |
|---|---|---|
| Data model | Custom entries + links | Standard SQL |
| Developer experience | Learn Holochain Rust framework | Write SQL |
| Storage | DHT replication | RLNC erasure coding |
| Adoption barrier | High (custom everything) | Low (it's just SQLite) |

"They built a framework. We built a database."

---

## Related Protocols

Protocols that won by being dumb:

```
TCP/IP     → move packets (no opinion on content)
DNS        → resolve names (no opinion on targets)
HTTP       → request/response (no opinion on payload)
SMTP       → deliver messages (no opinion on text)
BitTorrent → share files (no opinion on files)
Craftec    → store/retrieve data (no opinion on what it is)
```

Every protocol that won was the dumbest one in its category.

---

## Go-to-Market: Financial Network First

"Store your files on P2P" is abstract. "Send and receive money with no middleman" is instantly understood.

```
1. Financial network → proves CraftOBJ works, solves churn, builds node base
2. File storage      → CraftVFS on top of the same network
3. Developer platform → CraftSQL opens up to developers
4. Full ecosystem    → grows naturally
```

Bitcoin did exactly this. Started as money. Data protocols came later.

The financial use case bootstraps the network. Financial nodes stay online because their money depends on it. That's stronger motivation than altruism.

---

## Multichain Architecture — Chains Are Just I/O Ports

### The Insight

On-chain escrow contracts do almost nothing: receive USDC, emit event, release USDC. That's it. The actual business logic — balances, authorization, withdrawal limits, rebalancing — all lives in CraftSQL.

Blockchains are dumb wallets with events. CraftSQL is the computer.

```
Traditional:
  Logic lives ON each chain (smart contracts)
  → deploy on every chain
  → learn Solidity, Rust, Move, etc.
  → audit each separately
  → limited by each chain's VM

Craftec:
  Logic lives in CraftSQL (one place)
  → chains are just deposit/withdrawal addresses
  → one codebase, one audit
  → unlimited by any chain's constraints
```

### How It Works

Each supported chain has a multisig wallet (not even a smart contract in most cases) operated by gateway agents (CraftCPU). All logic runs in CraftSQL.

```
User deposits USDC on any chain
↓
chain-watcher agent sees it
↓
INSERT INTO deposits (did, amount, chain, tx_hash, confirmations)
VALUES ('did:alice', 100, 'ethereum', '0xabc...', 12);
↓
UPDATE balances SET amount = amount + 100 WHERE did = 'did:alice';
↓
Alice transfers to Bob (pure CraftSQL, instant, free)
↓
Bob requests withdrawal on Base
↓
INSERT INTO withdrawals (did, amount, chain, destination_addr, status)
VALUES ('did:bob', 50, 'base', '0xdef...', 'pending');
↓
withdrawal-processor agents read pending withdrawals
3-of-5 threshold signature → release from Base multisig
↓
Bob receives USDC on Base
```

### What the User Sees

```
Deposit:  "Send USDC to this address on [any chain]" → balance appears
Transfer: Pure CraftSQL. No chain involved. Instant. Free.
Withdraw: "Send my USDC to [any chain]" → arrives
```

Once money is inside Craftec, it's chain-agnostic. The CraftSQL balance doesn't know or care where the USDC came from.

### Per-Chain Deployment

| Chain | What's Deployed | Why First |
|---|---|---|
| Solana | Escrow program | Cheapest, fastest, existing expertise |
| Base | Multisig wallet | Largest L2 user base |
| Ethereum | Multisig wallet | Institutional money |
| Arbitrum | Multisig wallet | DeFi-heavy users |

Start with Solana. Add chains as demand appears. Each chain is just a new watched address + gateway agent. No new contract development needed.

### CCTP for Rebalancing

CCTP (Circle's Cross-Chain Transfer Protocol) becomes an internal treasury tool, not user-facing plumbing.

```
Problem: ETH escrow has $1M, Base escrow has $100
         User wants to withdraw $500 on Base

Solution: Rebalancer agent triggers CCTP
          ETH escrow → CCTP burn → mint on Base → Base escrow
          Now Base has $600, withdrawal succeeds
```

CCTP uses burn-and-mint (no wrapped tokens, no liquidity pools). Circle attests each transfer. Native USDC on both sides. Supports 15+ chains.

Craftec's rebalancer agent automates this entirely.

### Beyond USDC

The architecture isn't limited to USDC. Any asset, any chain, same SQL:

```sql
-- Gateway watches Bitcoin address
INSERT INTO deposits (did, amount, asset, chain, tx_hash)
VALUES ('did:alice', 0.5, 'BTC', 'bitcoin', 'abc...');

-- Gateway watches USDT on Tron
INSERT INTO deposits (did, amount, asset, chain, tx_hash)
VALUES ('did:bob', 200, 'USDT', 'tron', 'def...');
```

Any chain that can hold tokens and confirm transactions can be an I/O port. The gateway just needs to watch addresses and verify finality.

### Auditability

Every deposit and withdrawal logged in CraftSQL — public, verifiable. Mismatch between on-chain balance and CraftSQL balance is instantly detectable:

```sql
SELECT
  (SELECT SUM(amount) FROM deposits WHERE chain='ethereum')
  - (SELECT SUM(amount) FROM withdrawals WHERE chain='ethereum' AND status='complete')
  AS expected_onchain_balance;

-- Compare with actual on-chain balance
-- Any discrepancy = proof of fraud
```

This is the world's most transparent exchange: order book, balances, and audit trail are all public CraftSQL databases — with no company, no server, no single point of failure.

### The Architecture

```
Layer 1: Blockchains (ETH, Solana, Base, Arb, BTC...) = parking lots (hold assets)
Layer 2: Escrow wallets on each chain                  = entrance/exit gates
Layer 3: CraftCPU gateway agents                       = watchers, signers, rebalancers, auditors
Layer 4: Craftec financial chain (CraftSQL)             = the actual city where everything happens
Layer 5: CCTP                                          = highway between parking lots (rebalancing)
```

The multichain answer isn't making Craftec run on multiple chains. It's making multiple chains deposit into one Craftec.

---

## Data Growth

Storage grows linearly, not exponentially. A write only creates new CIDs for changed pages.

```
1 user, 10 transactions/day: ~1.5MB/year
1 million users: ~1.5TB/year
With 30x replication: ~45TB/year across the whole network
```

Natural pruning through churn: old unreferenced CIDs fade on the free tier. Active data heals. Forgotten data dies.

Users choose their own retention policy.

---

## The Principle

Everything is CIDs. Everything is pages. One protocol.

```
A file             = a CID
A database page    = a CID
A directory listing = a query on CIDs
A snapshot         = a saved CID
A filesystem       = a table of CIDs
An agent's state   = a CID
An agent's code    = a CID
A program          = a CID (with a derived key)
A ZK proof         = a CID
A website          = a folder of CIDs
```

The products organize CIDs into increasingly useful structures:

1. **craftec-core**: P2P connectivity, identity, crypto, erasure — the foundation
2. **CraftOBJ**: raw CIDs — store, retrieve, heal
3. **CraftSQL**: CIDs as B-tree pages — query, index, transact
4. **CraftVFS**: CraftSQL tables with path semantics — browse, mount, snapshot
5. **CraftSEC**: programs as CIDs with MPC-derived keys — validate, attest, sign
6. **CraftNET**: encrypted tunnels over the P2P network — VPN

Each product is a thin abstraction. The power comes from composition, not complexity.

---

## Priority Order

```
1. CraftOBJ — reliable store and retrieve (foundation)
2. CraftSQL — SQLite VFS on CraftOBJ (makes it useful)
3. CraftSEC — MPC threshold functions (the trust layer)
4. CraftCPU — persistent CPU agents (gateway for financial layer)
5. CraftVFS — filesystem on CraftSQL (makes it accessible)
6. CraftNET — decentralized VPN (when network is mature)
7. CraftGPU — persistent GPU agents (when network is mature)
```

CraftSEC is #3 because the financial network — the first product — requires MPC attestation for every transaction. Without CraftSEC, users can write anything to their chains. With it, every entry is cryptographically validated.

CraftCPU follows immediately because gateway agents depend on CraftSEC for threshold signing of cross-chain withdrawals.

---

## Repos

```
craftec/craftec-core  ← shared infrastructure (identity, crypto, erasure, P2P transport)
craftec/craftobj      ← object storage (depends on craftec-core)
craftec/craftsql      ← SQLite VFS (depends on craftobj)
craftec/craftsec      ← MPC threshold functions (depends on craftsql)
craftec/craftcpu      ← persistent CPU agents (depends on craftsql, craftsec)
craftec/craftgpu      ← persistent GPU agents (depends on craftsql, craftsec)
craftec/craftvfs      ← filesystem (depends on craftsql)
craftec/craftnet      ← decentralized VPN (depends on craftec-core)
```

One org. One repo per component. Separate repos enforce clean dependency boundaries.
