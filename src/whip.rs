use axum::{
    extract::State,
    response::IntoResponse,
    http::{StatusCode, header},
};
use std::sync::Arc;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::track::track_remote::TrackRemote;
use crate::AppState;

pub async fn whip_handler(
    State(state): State<Arc<AppState>>,
    body: String,
) -> impl IntoResponse {
    tracing::info!("Received WHIP offer");

    let clean_body = body.trim().trim_matches(|c| c == '"' || c == '\'' || c == '\n' || c == '\r').to_string();
    
    // Log the incoming SDP for diagnostics
    let candidate_count = clean_body.lines().filter(|l| l.trim().starts_with("a=candidate:")).count();
    tracing::info!("WHIP offer contains {} ICE candidate lines", candidate_count);
    if candidate_count == 0 {
        tracing::warn!("WHIP offer has ZERO candidates — BUTT client may not have gathered yet");
    }

    let offer = match RTCSessionDescription::offer(clean_body) {
        Ok(offer) => offer,
        Err(e) => {
            tracing::error!("WHIP SDP Processing failed: {:?}", e);
            return (StatusCode::BAD_REQUEST, [(header::CONTENT_TYPE, "text/plain")], format!("SDP Parse Error: {:?}", e)).into_response();
        }
    };
    
    // The server already knows its public IP via set_nat_1to1_ips (host candidate).
    // But we MUST add TURN for relay fallback — remote clients behind CGNAT
    // cannot reach our home IP directly without a relay path.
    let config = RTCConfiguration {
        ice_servers: vec![
            webrtc::ice_transport::ice_server::RTCIceServer {
                urls: vec!["turn:coturn:3478?transport=udp".to_owned()],
                username: "aurauser".to_owned(),
                credential: "aurapassword".to_owned(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let peer_connection = match state.api.new_peer_connection(config).await {
        Ok(pc) => Arc::new(pc),
        Err(e) => {
            tracing::error!("Failed to create WHIP peer connection: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, [(header::CONTENT_TYPE, "text/plain")], e.to_string()).into_response();
        }
    };

    // IMMEDIATELY initialize gathering promise before ICE starts
    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    // Log ICE diagnostics
    peer_connection.on_ice_connection_state_change(Box::new(move |state| {
        tracing::info!("WHIP ICE connection state changed: {:?}", state);
        Box::pin(async {})
    }));

    peer_connection.on_ice_candidate(Box::new(move |candidate| {
        if let Some(c) = candidate {
            tracing::info!("WHIP local ICE candidate generated: {}", c.to_string());
        } else {
            tracing::info!("WHIP ICE candidate gathering finished (nil candidate)");
        }
        Box::pin(async {})
    }));

    let tx = state.tx.clone();
    
    peer_connection.on_track(Box::new(move |track: Arc<TrackRemote>, _receiver, _| {
        let tx = tx.clone();
        Box::pin(async move {
            tracing::info!("Incoming RTP track started: {}", track.id());
            
            while let Ok((packet, _)) = track.read_rtp().await {
                // Broadcast the incoming RTP packet to all WHEP listeners
                // We don't care if there are no receivers, so we ignore the error
                let _ = tx.send(packet);
            }
            
            tracing::info!("Incoming RTP track ended: {}", track.id());
        })
    }));

    // Set the remote SessionDescription
    if let Err(e) = peer_connection.set_remote_description(offer).await {
        tracing::error!("Failed to set remote description: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, [(header::CONTENT_TYPE, "text/plain")], e.to_string()).into_response();
    }

    // Create an answer
    let answer = match peer_connection.create_answer(None).await {
        Ok(answer) => answer,
        Err(e) => {
            tracing::error!("Failed to create answer: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, [(header::CONTENT_TYPE, "text/plain")], e.to_string()).into_response();
        }
    };

    // Set the local description and start UDP listeners
    if let Err(e) = peer_connection.set_local_description(answer).await {
        tracing::error!("Failed to set local description: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, [(header::CONTENT_TYPE, "text/plain")], e.to_string()).into_response();
    }

    // Wait until ICE gathering is complete
    let _ = gather_complete.recv().await;

    let local_desc = match peer_connection.local_description().await {
        Some(desc) => desc,
        None => {
            tracing::error!("No local description available");
            return (StatusCode::INTERNAL_SERVER_ERROR, [(header::CONTENT_TYPE, "text/plain")], "No local description".to_string()).into_response();
        }
    };

    // Log the outgoing SDP for diagnostics
    let answer_candidate_count = local_desc.sdp.lines().filter(|l| l.trim().starts_with("a=candidate:")).count();
    tracing::info!("WHIP answer contains {} ICE candidate lines", answer_candidate_count);
    if answer_candidate_count == 0 {
        tracing::error!("CRITICAL: WHIP answer has ZERO candidates — BUTT will never connect!");
    }
    tracing::info!("Sending WHIP answer SDP:\n{}", local_desc.sdp);

    (
        StatusCode::CREATED,
        [
            (header::CONTENT_TYPE, "application/sdp"),
            (header::LOCATION, "/whip/resource"),
        ],
        local_desc.sdp,
    ).into_response()
}

