use super::*;
use crate::runtime::learned_method::{LearnedMethod, LearnedMethodState, MethodDecision};
use crate::runtime::learned_method_decide::{confirm, record_check, reject, review_code};
use crate::runtime::learned_method_edit::{deactivate, edit, restore};
use crate::runtime::learned_method_evidence::{collect, digest};
use crate::runtime::learned_method_generate::{Generator, Proposal};
use crate::runtime::learned_method_render::{self as render, Checked};
use crate::runtime::learned_method_render_changes as changed;
use crate::runtime::learned_method_render_decisions as decided;
use crate::runtime::learned_method_source::injected;
use crate::runtime::learned_method_store::LearnedMethodStore;
use crate::runtime::learned_method_verify::TaskOutcome;
use shacs_command::LearnCommandArgs;

const USAGE: &str = "Usage: /learn, /learn list, /learn show <id> [v<N>], /learn confirm <id>, /learn reject <id> v<N>, /learn verify <id>, /learn match <request>, /learn edit <id> v<N> purpose|applies <text>, /learn edit <id> v<N> premise|step <k> <text>, /learn edit <id> v<N> remove premise|step <k>, /learn deactivate <id> v<N>, or /learn restore <id> v<N>.";

/// A turn the user typed, as opposed to one the runtime wrote or injected;
/// both stored user turns and `/learn` itself are judged by this.
pub(super) fn owner_turn(message: &InboundMessage) -> bool {
    method::user_turn(message) && !injected(&message.metadata)
}

impl AgentLoop<'_> {
    pub(super) fn handle_learn_command(
        &mut self,
        args: LearnCommandArgs,
        message: &InboundMessage,
        session: Session,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let viewer = digest(&session.key);
        let content = if !owner_turn(message) {
            "Method learning is accepted only from user turns; nothing was changed.".to_owned()
        } else {
            match self.checked(&session) {
                Err(error) => render::store_error(error),
                Ok(methods) => self.learn(args, &session, &viewer, methods),
            }
        };
        self.publish_command_response(message, session, &content, None, "learn", true)
    }

    /// Runs one `/learn` command on a store that rechecked against its
    /// sessions.
    fn learn(
        &self,
        args: LearnCommandArgs,
        session: &Session,
        viewer: &str,
        methods: Vec<Checked>,
    ) -> String {
        let find = |id: &str| {
            methods
                .iter()
                .find(|checked| checked.method.method_id == id)
        };
        let missing = |id: &str| format!("No learned method {id}.");
        let now = || (viewer, now_iso());
        match args {
            LearnCommandArgs::Invalid => USAGE.to_owned(),
            LearnCommandArgs::Propose => self.propose(session, viewer),
            LearnCommandArgs::List => render::list(&methods),
            LearnCommandArgs::Show(id) => {
                find(&id).map_or_else(|| missing(&id), |checked| render::show(checked, viewer))
            }
            LearnCommandArgs::ShowVersion { method_id, version } => find(&method_id).map_or_else(
                || missing(&method_id),
                |checked| render::show_version(checked, viewer, version),
            ),
            LearnCommandArgs::Review(id) => match find(&id) {
                None => missing(&id),
                Some(checked) if checked.method.state == LearnedMethodState::Rejected => {
                    let version = checked.method.current_version;
                    format!(
                        "{id} v{version} was rejected and stays rejected. To change it, edit it into a new version (/learn edit {id} v{version} …) or make an earlier confirmed version current again (/learn restore {id} v<N>)."
                    )
                }
                Some(checked) if checked.method.state == LearnedMethodState::Confirmed => {
                    let version = checked.method.current_version;
                    format!(
                        "{id} v{version} is already confirmed. To stop using it until you confirm it again, send /learn deactivate {id} v{version}; to withdraw it for good, send /learn reject {id} v{version}."
                    )
                }
                Some(checked) => {
                    let shown = render::show(checked, viewer);
                    changed::review(&shown, &checked.method, &review_code(&checked.method))
                }
            },
            LearnCommandArgs::Confirm {
                method_id,
                version,
                code,
            } => self.change(
                session,
                &method_id,
                |method| confirm(method, version, &code, now()),
                |checked| changed::confirmed(&checked.method),
            ),
            LearnCommandArgs::Reject { method_id, version } => self.change(
                session,
                &method_id,
                |method| reject(method, version, now()),
                |checked| changed::rejected(&checked.method),
            ),
            LearnCommandArgs::Edit {
                method_id,
                version,
                part,
                text,
            } => self.change(
                session,
                &method_id,
                |method| edit(method, (version, part, text.as_deref()), now()),
                |checked| {
                    let shown = render::show(checked, viewer);
                    let method = &checked.method;
                    changed::edited(
                        method,
                        &changed::review(&shown, method, &review_code(method)),
                    )
                },
            ),
            LearnCommandArgs::Deactivate { method_id, version } => self.change(
                session,
                &method_id,
                |method| deactivate(method, version, now()),
                |checked| changed::deactivated(&checked.method),
            ),
            LearnCommandArgs::Restore { method_id, version } => self.change(
                session,
                &method_id,
                |method| restore(method, version, now()),
                |checked| changed::restored(&checked.method),
            ),
            LearnCommandArgs::Verify(id) => match find(&id) {
                None => missing(&id),
                Some(checked) => self.verify(session, viewer, &checked.method),
            },
            LearnCommandArgs::Match(request) => {
                let methods: Vec<LearnedMethod> =
                    methods.into_iter().map(|checked| checked.method).collect();
                match self.generator().match_request(&request, &methods) {
                    Ok(matched) => crate::runtime::learned_method_use::matched(&matched),
                    Err(reason) => {
                        format!("Matching did not finish: {reason}. Nothing was changed.")
                    }
                }
            }
        }
    }

    pub(super) fn generator(&self) -> Generator<'_> {
        Generator {
            client: self.client,
            model: &self.config.model,
            settings: &self.config.settings,
            retry_mode: self.config.retry_mode,
        }
    }

    fn propose(&self, session: &Session, viewer: &str) -> String {
        let registered = |name: &str| self.tools.has(name);
        match self
            .generator()
            .propose(collect(session, &registered), now_iso())
        {
            Proposal::Candidate(version) => LearnedMethodStore::new(&self.config.workspace)
                .add_candidate(*version)
                .and_then(|method| self.check(session, vec![method]))
                .map_or_else(render::store_error, |checked| {
                    checked
                        .first()
                        .map_or_else(String::new, |checked| render::created(checked, viewer))
                }),
            Proposal::NoCandidate(reason) => render::no_candidate(&reason),
            Proposal::Failed(reason) => render::failed(&reason),
        }
    }

    /// Applies `change` to the method as stored now, under the store lock,
    /// and checks the store against its sessions again before telling.
    fn change(
        &self,
        session: &Session,
        method_id: &str,
        change: impl FnOnce(&mut LearnedMethod) -> Result<(), String>,
        done: impl FnOnce(&Checked) -> String,
    ) -> String {
        let changed = LearnedMethodStore::new(&self.config.workspace).update(method_id, change);
        match changed {
            Err(error) => render::store_error(error),
            Ok(Err(reason)) => reason,
            Ok(Ok(_)) => match self.checked(session) {
                Ok(methods) => methods
                    .iter()
                    .find(|checked| checked.method.method_id == method_id)
                    .map_or_else(|| format!("No learned method {method_id}."), done),
                Err(error) => render::store_error(error),
            },
        }
    }

    /// Checks the task in this conversation against `method`.
    fn verify(&self, session: &Session, viewer: &str, method: &LearnedMethod) -> String {
        let id = &method.method_id;
        if method.state == LearnedMethodState::Rejected {
            return format!("{id} was rejected; it is not checked.");
        }
        let Some(version) = method.current() else {
            return format!("{id} has no readable version.");
        };
        let earlier: Vec<&MethodDecision> = method
            .decisions
            .iter()
            .filter(|decision| decision.version == version.version)
            .collect();
        let registered = |name: &str| self.tools.has(name);
        let evidence = collect(session, &registered);
        match self.generator().check_task(version, &earlier, evidence) {
            TaskOutcome::NotCounted(reason) => decided::not_counted(id, &reason),
            TaskOutcome::Failed(reason) => decided::check_failed(&reason),
            TaskOutcome::Record(kind, check, reason) => {
                let saved = LearnedMethodStore::new(&self.config.workspace).update(id, |method| {
                    record_check(method, (version.version, kind, check), (viewer, now_iso()))
                });
                match saved {
                    Err(error) => render::store_error(error),
                    Ok(Err(refused)) => refused,
                    Ok(Ok(method)) => match self.checked(session) {
                        Ok(_) => decided::task_checked(&method, kind, &reason),
                        Err(error) => render::store_error(error),
                    },
                }
            }
        }
    }
}
