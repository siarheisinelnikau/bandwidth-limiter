use bandwidth_limiter_control::{Action, Policy, Request, Response, Snapshot, VERSION};
pub trait Backend {
    fn snapshot(&self) -> Result<Snapshot, String>;
    fn apply(&self, policy: &Policy) -> Result<(), String>;
    fn pause(&self) -> Result<(), String>;
}
pub fn execute(backend: &impl Backend, request: Request, administrator: bool) -> Response {
    let result = (|| {
        if request.version != VERSION {
            return Err("Incompatible WFP control version".into());
        }
        if !matches!(request.action, Action::Status) && !administrator {
            return Err("Administrator authorization is required for WFP changes".into());
        }
        match request.action {
            Action::Status => {}
            Action::Apply { policy } => {
                policy.validate()?;
                backend.apply(&policy)?;
            }
            Action::Pause => backend.pause()?,
        }
        Ok(())
    })();
    let status = backend.snapshot();
    Response {
        version: VERSION,
        can_configure: administrator,
        status: status.as_ref().ok().cloned(),
        error: result.err().or_else(|| status.err()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Fake {
        changes: Cell<usize>,
    }
    impl Backend for Fake {
        fn snapshot(&self) -> Result<Snapshot, String> {
            Ok(Snapshot::default())
        }
        fn apply(&self, _: &Policy) -> Result<(), String> {
            self.changes.set(self.changes.get() + 1);
            Ok(())
        }
        fn pause(&self) -> Result<(), String> {
            self.changes.set(self.changes.get() + 1);
            Ok(())
        }
    }
    #[test]
    fn read_only_clients_cannot_mutate_and_bad_policies_do_not_pause() {
        let backend = Fake {
            changes: Cell::new(0),
        };
        assert!(
            execute(
                &backend,
                Request {
                    version: VERSION,
                    action: Action::Status
                },
                false
            )
            .status
            .is_some()
        );
        assert!(
            execute(
                &backend,
                Request {
                    version: VERSION,
                    action: Action::Pause
                },
                false
            )
            .error
            .is_some()
        );
        assert_eq!(backend.changes.get(), 0);
        assert!(
            execute(
                &backend,
                Request {
                    version: VERSION,
                    action: Action::Apply {
                        policy: Policy {
                            version: VERSION,
                            rules: vec![]
                        }
                    }
                },
                true
            )
            .error
            .is_some()
        );
        assert_eq!(backend.changes.get(), 0);
        assert!(
            execute(
                &backend,
                Request {
                    version: VERSION,
                    action: Action::Pause
                },
                true
            )
            .error
            .is_none()
        );
        assert_eq!(backend.changes.get(), 1);
    }
}
