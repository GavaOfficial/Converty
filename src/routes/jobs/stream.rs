//! SSE streaming for job progress

use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
};
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::Stream;
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::models::{JobStatus, ProgressUpdate};

use super::JobsState;

/// Stream personalizzato per progress di un job
pub struct JobProgressStream {
    job_id: Uuid,
    rx: BroadcastStream<ProgressUpdate>,
    initial_sent: bool,
    initial_update: Option<ProgressUpdate>,
    terminated: bool,
}

impl Stream for JobProgressStream {
    type Item = std::result::Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Se già terminato, non produrre più eventi
        if self.terminated {
            return Poll::Ready(None);
        }

        // Prima invia l'evento iniziale
        if !self.initial_sent {
            self.initial_sent = true;
            if let Some(update) = self.initial_update.take() {
                let json = serde_json::to_string(&update).unwrap_or_default();
                // Controlla se già terminale
                if update.status == JobStatus::Completed || update.status == JobStatus::Failed {
                    self.terminated = true;
                }
                return Poll::Ready(Some(Ok(Event::default().data(json))));
            }
        }

        // Poi ascolta nuovi eventi dal broadcast
        let rx = Pin::new(&mut self.rx);
        match rx.poll_next(cx) {
            Poll::Ready(Some(Ok(update))) => {
                if update.job_id == self.job_id {
                    let json = serde_json::to_string(&update).unwrap_or_default();
                    // Controlla se terminale
                    if update.status == JobStatus::Completed || update.status == JobStatus::Failed {
                        self.terminated = true;
                    }
                    Poll::Ready(Some(Ok(Event::default().data(json))))
                } else {
                    // Non è il nostro job, continua a pollare
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            Poll::Ready(Some(Err(_))) => {
                // Errore broadcast (lag), continua
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Stream SSE per monitorare il progress di un job in tempo reale
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}/progress",
    tag = "Jobs",
    params(
        ("id" = String, Path, description = "ID del job")
    ),
    responses(
        (status = 200, description = "Stream SSE con aggiornamenti progress", body = ProgressUpdate),
        (status = 404, description = "Job non trovato"),
    )
)]
pub async fn job_progress_stream(
    State(state): State<JobsState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>> {
    let job_id = Uuid::parse_str(&id).map_err(|_| AppError::JobNotFound(id.clone()))?;

    // Verifica che il job esista e ottieni stato iniziale
    let initial_update = {
        let q = state.queue.read().await;
        let job = q
            .get_job(&job_id)
            .await?
            .ok_or_else(|| AppError::JobNotFound(id.clone()))?;
        job.to_progress_update()
    };

    // Subscribe al broadcast channel
    let rx = state.progress_tx.subscribe();

    let stream = JobProgressStream {
        job_id,
        rx: BroadcastStream::new(rx),
        initial_sent: false,
        initial_update: Some(initial_update),
        terminated: false,
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
