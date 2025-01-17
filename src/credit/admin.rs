// ----- standard library imports
// ----- extra library imports
use axum::extract::{Json, Path, State};
use rust_decimal::Decimal;
use uuid::Uuid;
// ----- local modules
// ----- local modules
use crate::credit::{mint, Controller, Result};

/// --------------------------- List quotes
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ListQuotesReply {
    pub quotes: Vec<uuid::Uuid>,
}

pub async fn list_pending_quotes(State(ctrl): State<Controller>) -> Result<Json<ListQuotesReply>> {
    log::debug!("Received request to list pending quotes");

    let quotes = ctrl.quote_service.list_pending()?;
    Ok(Json(ListQuotesReply { quotes }))
}

pub async fn list_accepted_quotes(State(ctrl): State<Controller>) -> Result<Json<ListQuotesReply>> {
    log::debug!("Received request to list accepted quotes");

    let quotes = ctrl.quote_service.list_accepted()?;
    Ok(Json(ListQuotesReply { quotes }))
}

/// --------------------------- Look up request
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase", tag = "status")]
pub enum LookUpQuoteReply {
    Pending {
        id: Uuid,
        bill: String,
        endorser: String,
        tstamp: chrono::DateTime<chrono::Utc>,
    },
    Accepted {
        id: Uuid,
        bill: String,
        endorser: String,
        tstamp: chrono::DateTime<chrono::Utc>,
    },
    Declined {
        id: Uuid,
        bill: String,
        endorser: String,
        tstamp: chrono::DateTime<chrono::Utc>,
    },
}

impl std::convert::From<mint::Quote> for LookUpQuoteReply {
    fn from(quote: mint::Quote) -> Self {
        match quote {
            mint::Quote::Pending(request) => LookUpQuoteReply::Pending {
                id: request.id,
                bill: request.bill,
                endorser: request.endorser,
                tstamp: request.tstamp,
            },
            mint::Quote::Accepted(request, _) => LookUpQuoteReply::Accepted {
                id: request.id,
                bill: request.bill,
                endorser: request.endorser,
                tstamp: request.tstamp,
            },
            mint::Quote::Declined(request) => LookUpQuoteReply::Declined {
                id: request.id,
                bill: request.bill,
                endorser: request.endorser,
                tstamp: request.tstamp,
            },
        }
    }
}

pub async fn lookup_quote(
    State(ctrl): State<Controller>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<LookUpQuoteReply>> {
    log::debug!("Received mint quote lookup request for id: {}", id);

    let service = ctrl.quote_service.clone();
    let quote = service.lookup(id)?;
    let response = LookUpQuoteReply::from(quote);
    Ok(Json(response))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase", tag = "action")]
pub enum ResolveQuoteRequest {
    Decline,
    Accept {
        discount: Decimal,
        ttl: chrono::DateTime<chrono::Utc>,
    },
}

pub async fn resolve_quote(
    State(ctrl): State<Controller>,
    Path(id): Path<uuid::Uuid>,
    Json(req): Json<ResolveQuoteRequest>,
) -> Result<()> {
    log::debug!("Received mint quote resolve request for id: {}", id);

    let mut service = ctrl.quote_service.clone();
    match req {
        ResolveQuoteRequest::Decline => service.decline(id)?,
        ResolveQuoteRequest::Accept { discount, ttl } => service.accept(id, discount, ttl)?,
    }
    Ok(())
}
