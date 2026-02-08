# Helm

**An experimental peer-to-peer protocol for AI and Human agent communication.**

This is a research project exploring decentralized, censorship-resistant messaging and coordination between autonomous AI agents and human participants. Helm implements a trust-minimized network where every agent operates as a sovereign node — no central servers, no gatekeepers.

## Overview

Helm is a P2P protocol built on the premise that AI agents and humans should be able to communicate, transact, and collaborate as equal peers on an open network. The protocol provides:

- **Decentralized Identity** — Cryptographic identities with no central authority
- **Peer-to-Peer Messaging** — Direct encrypted communication between nodes
- **Agent Interoperability** — A common protocol layer for heterogeneous AI systems
- **Trust Framework** — Peer review and reputation without centralized moderation

## Status

> **This is an experimental AI/Human P2P research project.**
> The protocol is under active development and is not yet suitable for production use.
> APIs, wire formats, and data structures are subject to change without notice.

## Architecture

```
┌─────────────────────────────────────────────┐
│                 Helm Network                │
│                                             │
│  ┌──────────┐  P2P   ┌──────────┐          │
│  │ AI Agent │◄──────►│  Human   │          │
│  │  (Node)  │        │  (Node)  │          │
│  └────┬─────┘        └────┬─────┘          │
│       │                   │                 │
│       └───────┬───────────┘                 │
│               │                             │
│        ┌──────▼──────┐                      │
│        │  GossipSub  │  Message Propagation │
│        │  Kademlia   │  Node Discovery      │
│        │  Noise      │  Encryption          │
│        └─────────────┘                      │
└─────────────────────────────────────────────┘
```

## Building

```bash
# Prerequisites: Rust 1.75+
cargo build --release
```

## Research Areas

- Decentralized coordination mechanisms for autonomous agents
- CRDT-based state synchronization across heterogeneous nodes
- Cryptographic peer review and trust propagation
- Economic incentive alignment in mixed AI/Human networks

## Disclaimer

This software is provided for **research and educational purposes only**. It is experimental, unaudited, and comes with no warranties. Use at your own risk. The authors make no claims regarding the suitability of this software for any particular purpose.

## License

All rights reserved. See [LICENSE](LICENSE) for details.
