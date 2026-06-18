# AuraWebRTC

AuraWebRTC is a production-ready, ultra-low-latency audio streaming server written in Go. It bypasses legacy container demuxing (like Icecast, Ogg, HTTP Source) by acting as a highly optimized, native WebRTC Selective Forwarding Unit (SFU) specifically tailored for live audio distribution.

## System Prerequisites

- **Go**: Version 1.21 or higher.
- **BUTT**: Broadcast Using This Tool (for the WHIP ingest client).
- **Network**: Port 8080 available for the HTTP server, and open UDP routing for WebRTC ICE (Stun/Turn).

## Architecture

AuraWebRTC isolates the ingest and egress streams via an internal lock-free ring buffer multiplexer. 

*   **Ingest Pipeline (WHIP):** Complies with the WebRTC HTTP Ingestion Protocol (RFC 9435). The server exposes an HTTP POST endpoint. A source client (like BUTT) sends a session description (SDP Offer). The server allocates a `PeerConnection`, answers with `201 Created` and a local SDP Answer, and begins receiving pre-packetized SRTP/UDP Opus audio frames.
*   **Buffer Hub:** A thread-safe multiplexer handles fan-out. It pushes UDP packets directly into subscriber channels using non-blocking writes. This ensures that slow readers never stall the incoming read loops or the system GC.
*   **Egress Pipeline (WHEP):** Browser clients connect via a custom WebRTC signaling endpoint (similar to WHEP). A native WebRTC `PeerConnection` is negotiated, mapping the user's connection to the buffer hub via a `TrackLocalStaticRTP`. The audio is delivered securely via DTLS/SRTP directly to an HTML5 `<audio>` element.

## Production Hardening Checklist

*   [x] **Network Primitives:** UDP MTU bounds are explicitly capped at `1200` bytes inside the Pion SettingEngine to eliminate UDP fragmentation down typical ISP pipes.
*   [x] **Zero-Allocation Strategies:** RTP packet processing operates within a continuous stream; the architecture prevents slice re-allocation overhead per subscriber frame push.
*   [x] **Non-Blocking Drop Policies:** If a browser listener's buffer channel fills due to local network congestion, frames are dropped instantly (`select` with `default:`). **Slow clients cannot crash the ingest routine**.
*   [x] **Single-Pass ICE Signaling:** Enforces `webrtc.GatheringCompletePromise()` prior to returning an SDP Answer to the HTTP client, eliminating trickle-ICE race conditions across standard corporate NATs.
*   [ ] **ulimit Verification:** When running in production, ensure the OS file descriptor limit (`ulimit -n`) is set high enough (e.g., `65535`) to support the expected concurrent WebRTC connections.

---

## Quick Start Guide

### 1. Start the Server

```bash
go mod tidy
go run ./cmd/server
```

### 2. Configure Ingest (BUTT)

1. Open **BUTT (Broadcast Using This Tool)**.
2. Go to **Settings > Server**.
3. Click **Add** to create a new server profile.
4. **Name**: AuraWebRTC Local
5. **Type**: `WHIP`
6. **Address (URL)**: `http://localhost:8080/whip/ingest`
7. Click **Save**.
8. Go to **Settings > Audio**.
9. **Codec**: `Opus`
10. **Sample Rate**: `48000 Hz` (48kHz)
11. **Channels**: `Stereo`
12. **Bitrate**: Select your desired quality (e.g., `128k`)
13. Return to the main window and hit the "Play/Record" icon to start streaming!

### 3. Connect a Listener

Open the included `index.html` file in any modern web browser. Click "Connect & Listen Live" to connect to the Egress Pipeline and hear the audio stream with sub-second latency.
