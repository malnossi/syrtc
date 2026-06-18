# Project Aura Architecture & Agent Guidelines

## 1. System Overview
Aura is an ultra-low latency, real-time live audio streaming server that ingests a professional broadcast stream via WHIP (WebRTC HTTP Ingestion Protocol) and distributes it instantly to multiple web clients via WHEP (WebRTC HTTP Egress Protocol). 

The system is engineered in Rust for maximum memory efficiency, zero garbage collection latency spikes, and predictable sub-second audio delivery.

## 2. Technical Stack
- **Language:** Rust (Stable edition)
- **Asynchronous Runtime:** `tokio` (Multi-threaded scheduler)
- **WebRTC Stack:** `webrtc` crate (The native Rust port of the Pion network stack)
- **HTTP/Signaling Server:** `axum` or `warp` (High-performance async routing)
- **Reverse Proxy:** Caddy (Handles Let's Encrypt SSL termination and serves static assets)
- **NAT Traversal Relay:** Coturn (Provides fallback STUN/TURN handling over port 3478)

## 3. Core Architectural Requirements

### A. Single-Port UDP Multiplexing
To accommodate restrictive home routers, the server must handle ALL WebRTC media traffic over a single incoming UDP port (`50000`). This is achieved by creating a standalone async UDP socket connection bound via `webrtc::api::setting_engine::SettingEngine::set_ice_mux`. Both WHIP and WHEP connections must share this exact same multiplexing instance.

### B. Perfect Listener Synchronization (Zero-Drift Loop)
To ensure all connected listeners hear the audio at the exact same physical moment without lag accumulating over time:
- Ingested Opus audio frames must be written to a lock-free, asynchronous broadcast ring buffer (e.g., `tokio::sync::broadcast`).
- Each new WHEP subscriber spawns a lightweight async worker task that reads immediately from the head of this broadcast channel.
- If a slow client drops behind due to cellular network degradation, the server will immediately drop obsolete packets for that client rather than buffering them, keeping all users perfectly synced to the live edge.

### C. 1-to-1 NAT IP Mapping
The `SettingEngine` must parse a provided `EXTERNAL_IP` environment variable (resolving No-IP domains via async DNS lookups). This resolved public IPv4 address must be forcefully injected into the server's local host ICE candidates using `set_nat_1to1_ips`.

## 4. Repository Structure Expected
- `src/main.rs`: Application entrypoint, infrastructure initialization, and server coordination loop.
- `src/whip.rs`: WHIP ingestion router handlers (handles BUTT connections and RTP track extraction).
- `src/whep.rs`: WHEP subscriber distribution handlers (tracks downstream peer connections).
- `index.html`: Optimized HTML5/WebKit-compatible frontend client.
- `Caddyfile`: Reverse proxy orchestration rules.
- `docker-compose.yml`: Local multi-container infrastructure stack definitions.