use axum::{
    routing::post,
    Router,
};
use std::env;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};
use tower_http::cors::{Any, CorsLayer};
use webrtc::api::APIBuilder;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::ice::udp_mux::{UDPMuxDefault, UDPMuxParams};
use webrtc::ice::udp_network::UDPNetwork;
use webrtc::ice_transport::ice_candidate_type::RTCIceCandidateType;
use webrtc::rtp::packet::Packet;

mod whip;
mod whep;

pub struct AppState {
    pub api: webrtc::api::API,
    pub tx: broadcast::Sender<Packet>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting Aura WebRTC server");

    let mut setting_engine = SettingEngine::default();
    
    // DISABLE mDNS! Docker does not support multicast natively. 
    // If left enabled, it breaks host candidate generation and NAT 1:1.
    setting_engine.set_ice_multicast_dns_mode(webrtc::ice::mdns::MulticastDnsMode::Disabled);

    // 1. Setup UDP Mux on port 50000
    let udp_socket = match UdpSocket::bind("0.0.0.0:50000").await {
        Ok(socket) => {
            tracing::info!("Successfully bound UDP socket to :50000 for ICE Multiplexing");
            socket
        }
        Err(e) => {
            tracing::error!("Failed to bind UDP socket to :50000: {}", e);
            return Err(e.into());
        }
    };
    let udp_mux = UDPMuxDefault::new(UDPMuxParams::new(udp_socket));
    setting_engine.set_udp_network(UDPNetwork::Muxed(udp_mux as Arc<dyn webrtc::ice::udp_mux::UDPMux + Send + Sync>));

    // 2. Lookup EXTERNAL_IP and set NAT 1:1 IPs with Retry Loop
    if let Ok(external_ip) = env::var("EXTERNAL_IP") {
        tracing::info!("Parsed EXTERNAL_IP environment variable: {}", external_ip);
        
        let mut attempts = 0;
        let max_attempts = 5;
        let mut resolved_ip = None;

        while attempts < max_attempts {
            tracing::info!("DNS lookup begins for {} (Attempt {}/{})", external_ip, attempts + 1, max_attempts);
            match tokio::net::lookup_host(format!("{}:0", external_ip)).await {
                Ok(mut resolved) => {
                    if let Some(addr) = resolved.next() {
                        tracing::info!("DNS lookup succeeds: resolved {} to {}", external_ip, addr.ip());
                        resolved_ip = Some(addr.ip().to_string());
                        break;
                    } else {
                        tracing::warn!("DNS lookup succeeded but returned no addresses for {}", external_ip);
                    }
                }
                Err(e) => {
                    tracing::warn!("DNS lookup failed for {}: {}", external_ip, e);
                }
            }
            
            attempts += 1;
            if attempts < max_attempts {
                tracing::info!("Waiting 2 seconds before retrying DNS lookup...");
                sleep(Duration::from_secs(2)).await;
            }
        }

        if let Some(ip) = resolved_ip {
            setting_engine.set_nat_1to1_ips(vec![ip], RTCIceCandidateType::Host);
        } else {
            tracing::error!("Failed to resolve EXTERNAL_IP after {} attempts. Continuing without 1:1 NAT mapping.", max_attempts);
        }
    } else {
        tracing::info!("No EXTERNAL_IP environment variable provided, skipping 1:1 NAT mapping.");
    }

    // 3. Create API with registered codecs
    // CRITICAL: Without registering codecs, the MediaEngine is empty and 
    // the server rejects all media lines with m=audio 0 (port 0 = rejected).
    let mut media_engine = webrtc::api::media_engine::MediaEngine::default();
    media_engine.register_default_codecs()?;

    let api = APIBuilder::new()
        .with_setting_engine(setting_engine)
        .with_media_engine(media_engine)
        .build();

    // 4. Create Broadcast Channel (1024 packets buffer)
    let (tx, _rx) = broadcast::channel(1024);

    let state = Arc::new(AppState {
        api,
        tx,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/whip/ingest", post(whip::whip_handler))
        .route("/whep/egress", post(whep::whep_handler))
        .layer(cors)
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind("0.0.0.0:8080").await {
        Ok(l) => {
            tracing::info!("Successfully bound Axum HTTP server to :8080");
            l
        }
        Err(e) => {
            tracing::error!("Failed to bind Axum HTTP server to :8080: {}", e);
            return Err(e.into());
        }
    };
    axum::serve(listener, app).await?;

    Ok(())
}
