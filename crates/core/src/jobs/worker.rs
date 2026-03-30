use std::{sync::Arc, time::Duration};

use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};

use crate::{
    Error,
    event::EventService,
    jobs::{JobRepository, JobService},
    repository::{Repository, transaction},
};

pub struct JobWorker {
    job_service: Arc<dyn JobService>,
    repository: Arc<dyn Repository>,
    job_repo: Arc<dyn JobRepository>,
    poll_interval: Duration,
    event_service: Arc<dyn EventService>,
}

impl JobWorker {
    pub fn new(
        job_service: Arc<dyn JobService>,
        repository: Arc<dyn Repository>,
        job_repo: Arc<dyn JobRepository>,
        poll_interval: Duration,
        event_service: Arc<dyn EventService>,
    ) -> Self {
        Self {
            job_service,
            repository,
            job_repo,
            poll_interval,
            event_service,
        }
    }
}

impl IntoSubsystem<Error> for JobWorker {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        let job_repo = self.job_repo;
        let repository = self.repository;
        let job_service = self.job_service;
        let poll_interval = self.poll_interval;
        let event_service = self.event_service;

        // Crash recovery: reset any jobs left running from a previous crash.
        // Uses ? intentionally — if DB is unreachable at startup, the error
        // propagates to the keepalive loop, which triggers the wrapper's check.
        let reset = transaction(&*repository, |tx| {
            let job_repo = job_repo.clone();
            Box::pin(async move { job_repo.reset_running_to_pending(tx).await })
        })
        .await?;

        if reset > 0 {
            tracing::warn!("reset {} running jobs to pending after startup", reset);
            event_service.notify_jobs_changed();
        }

        let mut counter: u32 = 0;
        loop {
            tokio::select! {
                () = subsys.on_shutdown_requested() => {
                    tracing::info!("JobWorker shutting down...");
                    break;
                }
                () = async {} => {
                    let mut job_processed = false;
                    if counter == 0 {
                        // Claim the next pending job.
                        let job = {
                            let job_repo = job_repo.clone();
                            match transaction(&*repository, |tx| {
                                Box::pin(async move { job_repo.claim_next(tx).await })
                            })
                            .await
                            {
                                Ok(j) => j,
                                Err(e) if e.is_transient() => {
                                    tracing::warn!("DB unavailable in worker (claim_next), pausing 10s: {e}");
                                    tokio::time::sleep(Duration::from_secs(10)).await;
                                    continue;
                                }
                                Err(e) => return Err(e),
                            }
                        };

                        match job {
                            None => {}
                            Some(job) => {
                                job_processed = true;
                                let job_type = job.job_type.clone();
                                let payload = job.payload.clone();

                                match job_service.dispatch(&job_type, payload).await {
                                    Ok(()) => {
                                        let job_repo = job_repo.clone();
                                        match transaction(&*repository, |tx| {
                                            let job = job.clone();
                                            Box::pin(async move { job_repo.complete(tx, job).await })
                                        })
                                        .await
                                        {
                                            Ok(_) => {}
                                            Err(e) if e.is_transient() => {
                                                tracing::warn!("DB unavailable in worker (complete), pausing 10s: {e}");
                                                tokio::time::sleep(Duration::from_secs(10)).await;
                                                continue;
                                            }
                                            Err(e) => return Err(e),
                                        }
                                        event_service.notify_jobs_changed();
                                    }
                                    Err(e) => {
                                        tracing::error!(job_type, error = %e, "job handler failed");
                                        let job_repo = job_repo.clone();
                                        match transaction(&*repository, |tx| {
                                            let job = job.clone();
                                            Box::pin(async move {
                                                job_repo.fail(tx, job, e.to_string()).await
                                            })
                                        })
                                        .await
                                        {
                                            Ok(_) => {}
                                            Err(e) if e.is_transient() => {
                                                tracing::warn!("DB unavailable in worker (fail), pausing 10s: {e}");
                                                tokio::time::sleep(Duration::from_secs(10)).await;
                                                continue;
                                            }
                                            Err(e) => return Err(e),
                                        }
                                        event_service.notify_jobs_changed();
                                    }
                                }
                            }
                        } // end match job
                    } // end if counter == 0

                    if !job_processed {
                        counter += 1;
                        #[expect(clippy::cast_possible_truncation, reason = "poll interval in seconds fits in u32; no sane interval exceeds ~136 years")]
                        let poll_secs = poll_interval.as_secs() as u32;
                        if counter >= poll_secs {
                            counter = 0;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Error, RepositoryError};

    #[test]
    fn transient_connection_error_is_transient() {
        let e = Error::RepositoryError(RepositoryError::Connection("gone".into()));
        assert!(e.is_transient(), "connection error must be transient");
    }

    #[test]
    fn infrastructure_error_is_not_transient() {
        let e = Error::Infrastructure("bad query".into());
        assert!(!e.is_transient(), "infrastructure error must not be transient");
    }
}
