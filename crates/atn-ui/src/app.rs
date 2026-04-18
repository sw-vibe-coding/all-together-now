use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use yew::prelude::*;

use crate::wiki::WikiBrowser;

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct AgentInfo {
    id: String,
    name: String,
    role: String,
}

#[derive(Clone, PartialEq)]
enum ViewMode {
    Agents,
    Wiki,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
struct SpawnSpecForm {
    name: String,
    role: String,
    transport: String,
    host: Option<String>,
    user: Option<String>,
    working_dir: String,
    project: Option<String>,
    agent: String,
    agent_args: Option<String>,
}

impl Default for SpawnSpecForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            role: "worker".to_string(),
            transport: "local".to_string(),
            host: None,
            user: None,
            working_dir: String::new(),
            project: None,
            agent: "claude".to_string(),
            agent_args: None,
        }
    }
}

impl SpawnSpecForm {
    fn is_remote(&self) -> bool {
        self.transport != "local"
    }

    fn missing_fields(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if self.name.trim().is_empty() {
            m.push("name");
        }
        if self.working_dir.trim().is_empty() {
            m.push("working_dir");
        }
        if self.agent.trim().is_empty() {
            m.push("agent");
        }
        if self.is_remote() {
            if self.host.as_deref().map(str::trim).unwrap_or("").is_empty() {
                m.push("host");
            }
            if self.user.as_deref().map(str::trim).unwrap_or("").is_empty() {
                m.push("user");
            }
        }
        m
    }

    fn preview(&self) -> String {
        let tail = match self.agent_args.as_deref() {
            Some(a) if !a.trim().is_empty() => format!("{} {}", self.agent, a.trim()),
            _ => self.agent.clone(),
        };
        let dir = if self.working_dir.is_empty() {
            "<dir>".to_string()
        } else {
            self.working_dir.clone()
        };
        let agent = if tail.is_empty() {
            "<agent>".to_string()
        } else {
            tail
        };
        let inner = format!("cd {dir} && {agent}");
        if self.transport == "local" {
            return inner;
        }
        let user = self.user.as_deref().unwrap_or("<user>");
        let host = self.host.as_deref().unwrap_or("<host>");
        let name = if self.name.is_empty() {
            "<name>"
        } else {
            self.name.as_str()
        };
        format!(
            "{bin} {user}@{host} -- tmux new-session -A -s atn-{name} '{inner}'",
            bin = self.transport,
        )
    }
}

#[function_component(App)]
pub fn app() -> Html {
    let agents = use_state(Vec::<AgentInfo>::new);
    let selected = use_state(|| None::<String>);
    let view = use_state(|| ViewMode::Agents);

    // Fetch agent list on mount.
    {
        let agents = agents.clone();
        use_effect_with((), move |_| {
            fetch_agents(agents);
        });
    }

    let on_select = {
        let selected = selected.clone();
        Callback::from(move |id: String| {
            selected.set(Some(id));
        })
    };

    let on_view_agents = {
        let view = view.clone();
        Callback::from(move |_: MouseEvent| view.set(ViewMode::Agents))
    };

    let on_view_wiki = {
        let view = view.clone();
        Callback::from(move |_: MouseEvent| view.set(ViewMode::Wiki))
    };

    let current_agent = (*selected)
        .clone()
        .or_else(|| agents.first().map(|a| a.id.clone()));

    let show_dialog = use_state(|| false);

    let on_new_agent = {
        let show_dialog = show_dialog.clone();
        Callback::from(move |_: MouseEvent| show_dialog.set(true))
    };

    let on_dialog_close = {
        let show_dialog = show_dialog.clone();
        let agents = agents.clone();
        Callback::from(move |created: bool| {
            show_dialog.set(false);
            if created {
                fetch_agents(agents.clone());
            }
        })
    };

    html! {
        <div class="app">
            <header>
                <h1>{ "All Together Now" }</h1>
                <span class="agent-count">
                    { format!("{} agent{}", agents.len(), if agents.len() != 1 { "s" } else { "" }) }
                </span>
                <button class="btn-new-agent" onclick={on_new_agent.clone()}>
                    { "+ New Agent" }
                </button>
                <div class="view-tabs">
                    <button
                        class={classes!("view-tab", (*view == ViewMode::Agents).then_some("active"))}
                        onclick={on_view_agents}
                    >{ "Agents" }</button>
                    <button
                        class={classes!("view-tab", (*view == ViewMode::Wiki).then_some("active"))}
                        onclick={on_view_wiki}
                    >{ "Wiki" }</button>
                </div>
            </header>
            if *view == ViewMode::Agents {
                if agents.is_empty() {
                    <EmptyAgentsState on_new_agent={on_new_agent.clone()} />
                } else {
                    <nav class="agent-tabs">
                        { for agents.iter().map(|a| {
                            let id = a.id.clone();
                            let is_active = current_agent.as_deref() == Some(&a.id);
                            let on_click = {
                                let on_select = on_select.clone();
                                let id = id.clone();
                                Callback::from(move |_: MouseEvent| on_select.emit(id.clone()))
                            };
                            html! {
                                <button
                                    class={classes!("tab", is_active.then_some("active"))}
                                    onclick={on_click}
                                >
                                    { &a.name }
                                    <span class="tab-role">{ &a.role }</span>
                                </button>
                            }
                        })}
                    </nav>
                    if let Some(agent_id) = current_agent {
                        <AgentPanel id={agent_id} />
                    }
                }
            } else {
                <WikiBrowser />
            }
            if *show_dialog {
                <NewAgentDialog on_close={on_dialog_close} />
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct NewAgentDialogProps {
    on_close: Callback<bool>,
}

#[function_component(NewAgentDialog)]
fn new_agent_dialog(props: &NewAgentDialogProps) -> Html {
    let form = use_state(SpawnSpecForm::default);
    let error = use_state(String::new);
    let submitting = use_state(|| false);

    let update_field = |field: fn(&mut SpawnSpecForm, String), form: UseStateHandle<SpawnSpecForm>| {
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            let mut next = (*form).clone();
            field(&mut next, input.value());
            form.set(next);
        })
    };

    let update_select = |field: fn(&mut SpawnSpecForm, String), form: UseStateHandle<SpawnSpecForm>| {
        Callback::from(move |e: Event| {
            let input: web_sys::HtmlSelectElement = e.target_unchecked_into();
            let mut next = (*form).clone();
            field(&mut next, input.value());
            form.set(next);
        })
    };

    let missing = form.missing_fields();
    let preview = form.preview();
    let is_remote = form.is_remote();

    let on_cancel = {
        let on_close = props.on_close.clone();
        Callback::from(move |_: MouseEvent| on_close.emit(false))
    };

    let on_backdrop = {
        let on_close = props.on_close.clone();
        Callback::from(move |e: MouseEvent| {
            if let Some(target) = e.target() {
                let el: Option<web_sys::Element> = target.dyn_into().ok();
                if let Some(el) = el
                    && el.class_name().contains("dialog-backdrop")
                {
                    on_close.emit(false);
                }
            }
        })
    };

    let on_submit = {
        let form = form.clone();
        let error = error.clone();
        let submitting = submitting.clone();
        let on_close = props.on_close.clone();
        Callback::from(move |_: MouseEvent| {
            let spec = (*form).clone();
            let missing = spec.missing_fields();
            if !missing.is_empty() {
                error.set(format!("missing: {}", missing.join(", ")));
                return;
            }
            submitting.set(true);
            let error = error.clone();
            let submitting = submitting.clone();
            let on_close = on_close.clone();
            submit_spawn_spec(spec, move |outcome| {
                submitting.set(false);
                match outcome {
                    Ok(()) => on_close.emit(true),
                    Err(msg) => error.set(msg),
                }
            });
        })
    };

    html! {
        <div class="dialog-backdrop" onclick={on_backdrop}>
            <div class="dialog new-agent-dialog" onclick={Callback::from(|e: MouseEvent| e.stop_propagation())}>
                <h2>{ "New Agent" }</h2>
                <div class="form-row">
                    <label>{ "name" }</label>
                    <input
                        type="text"
                        placeholder="e.g. worker-hlasm"
                        value={form.name.clone()}
                        oninput={update_field(|f, v| f.name = v, form.clone())}
                    />
                </div>
                <div class="form-row">
                    <label>{ "role" }</label>
                    <select
                        value={form.role.clone()}
                        onchange={update_select(|f, v| f.role = v, form.clone())}
                    >
                        <option value="worker" selected={form.role == "worker"}>{ "worker" }</option>
                        <option value="coordinator" selected={form.role == "coordinator"}>{ "coordinator" }</option>
                        <option value="qa" selected={form.role == "qa"}>{ "qa" }</option>
                        <option value="pm" selected={form.role == "pm"}>{ "pm" }</option>
                    </select>
                </div>
                <div class="form-row">
                    <label>{ "transport" }</label>
                    <select
                        value={form.transport.clone()}
                        onchange={update_select(|f, v| f.transport = v, form.clone())}
                    >
                        <option value="local" selected={form.transport == "local"}>{ "local" }</option>
                        <option value="mosh" selected={form.transport == "mosh"}>{ "mosh" }</option>
                        <option value="ssh" selected={form.transport == "ssh"}>{ "ssh" }</option>
                    </select>
                </div>
                <div class="form-row">
                    <label>{ "user" }</label>
                    <input
                        type="text"
                        placeholder="devh1"
                        disabled={!is_remote}
                        value={form.user.clone().unwrap_or_default()}
                        oninput={update_field(|f, v| f.user = if v.is_empty() { None } else { Some(v) }, form.clone())}
                    />
                    <label>{ "host" }</label>
                    <input
                        type="text"
                        placeholder="queenbee"
                        disabled={!is_remote}
                        value={form.host.clone().unwrap_or_default()}
                        oninput={update_field(|f, v| f.host = if v.is_empty() { None } else { Some(v) }, form.clone())}
                    />
                </div>
                <div class="form-row">
                    <label>{ "working dir" }</label>
                    <input
                        type="text"
                        placeholder="/home/devh1/work/hlasm"
                        value={form.working_dir.clone()}
                        oninput={update_field(|f, v| f.working_dir = v, form.clone())}
                    />
                </div>
                <div class="form-row">
                    <label>{ "project" }</label>
                    <input
                        type="text"
                        placeholder="(optional label)"
                        value={form.project.clone().unwrap_or_default()}
                        oninput={update_field(|f, v| f.project = if v.is_empty() { None } else { Some(v) }, form.clone())}
                    />
                </div>
                <div class="form-row">
                    <label>{ "agent" }</label>
                    <input
                        type="text"
                        placeholder="claude | codex | opencode | ..."
                        value={form.agent.clone()}
                        oninput={update_field(|f, v| f.agent = v, form.clone())}
                    />
                </div>
                <div class="form-row">
                    <label>{ "args" }</label>
                    <input
                        type="text"
                        placeholder="(optional)"
                        value={form.agent_args.clone().unwrap_or_default()}
                        oninput={update_field(|f, v| f.agent_args = if v.is_empty() { None } else { Some(v) }, form.clone())}
                    />
                </div>
                <div class="form-preview">
                    <code>{ preview }</code>
                </div>
                if !error.is_empty() {
                    <div class="form-error">{ (*error).clone() }</div>
                } else if !missing.is_empty() {
                    <div class="form-missing">
                        { format!("missing: {}", missing.join(", ")) }
                    </div>
                }
                <div class="form-actions">
                    <button class="btn-cancel" onclick={on_cancel} disabled={*submitting}>
                        { "Cancel" }
                    </button>
                    <button
                        class="btn-create"
                        onclick={on_submit}
                        disabled={!missing.is_empty() || *submitting}
                    >
                        { if *submitting { "Creating..." } else { "Create" } }
                    </button>
                </div>
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct EmptyAgentsStateProps {
    on_new_agent: Callback<MouseEvent>,
}

#[function_component(EmptyAgentsState)]
fn empty_agents_state(props: &EmptyAgentsStateProps) -> Html {
    html! {
        <div class="empty-state">
            <h2>{ "No agents yet" }</h2>
            <p>
                { "ATN is running but no agents are configured. Add one to start—\
                   pick a host (local or remote), working directory, and the agent CLI to run." }
            </p>
            <button class="btn-new-agent-primary" onclick={props.on_new_agent.clone()}>
                { "+ New Agent" }
            </button>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct AgentPanelProps {
    id: String,
}

#[function_component(AgentPanel)]
fn agent_panel(props: &AgentPanelProps) -> Html {
    let input_ref = use_node_ref();
    let input_value = use_state(String::new);
    let agent_id = props.id.clone();

    let on_input = {
        let input_value = input_value.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            input_value.set(input.value());
        })
    };

    let on_send = {
        let input_value = input_value.clone();
        let input_ref = input_ref.clone();
        let agent_id = agent_id.clone();
        Callback::from(move |_: MouseEvent| {
            let text = (*input_value).clone();
            if !text.is_empty() {
                send_input(&agent_id, &text);
                input_value.set(String::new());
                if let Some(el) = input_ref.cast::<web_sys::HtmlInputElement>() {
                    let _ = el.focus();
                }
            }
        })
    };

    let on_keydown = {
        let input_value = input_value.clone();
        let agent_id = agent_id.clone();
        Callback::from(move |e: KeyboardEvent| {
            if e.key() == "Enter" {
                let text = (*input_value).clone();
                if !text.is_empty() {
                    send_input(&agent_id, &text);
                    input_value.set(String::new());
                }
            }
        })
    };

    let on_ctrl_c = {
        let agent_id = agent_id.clone();
        Callback::from(move |_: MouseEvent| {
            send_ctrl_c(&agent_id);
        })
    };

    html! {
        <div class="panel">
            <div id={format!("terminal-{}", props.id)}></div>
            <div class="controls">
                <input
                    ref={input_ref}
                    type="text"
                    placeholder={format!("Command for {}...", props.id)}
                    value={(*input_value).clone()}
                    oninput={on_input}
                    onkeydown={on_keydown}
                />
                <button class="btn-send" onclick={on_send}>{ "Send" }</button>
                <button class="btn-ctrl-c" onclick={on_ctrl_c}>{ "^C" }</button>
            </div>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn fetch_agents(agents: UseStateHandle<Vec<AgentInfo>>) {
    use gloo_net::http::Request;
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(resp) = Request::get("/api/agents").send().await {
            if let Ok(list) = resp.json::<Vec<AgentInfo>>().await {
                agents.set(list);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_agents(_agents: UseStateHandle<Vec<AgentInfo>>) {}

#[cfg(target_arch = "wasm32")]
fn send_input(agent_id: &str, text: &str) {
    use gloo_net::http::Request;
    let url = format!("/api/agents/{}/input", agent_id);
    let body = serde_json::json!({ "text": text }).to_string();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = Request::post(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .unwrap()
            .send()
            .await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn send_input(_agent_id: &str, _text: &str) {}

#[cfg(target_arch = "wasm32")]
fn send_ctrl_c(agent_id: &str) {
    use gloo_net::http::Request;
    let url = format!("/api/agents/{}/ctrl-c", agent_id);
    wasm_bindgen_futures::spawn_local(async move {
        let _ = Request::post(&url).send().await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn send_ctrl_c(_agent_id: &str) {}

#[cfg(target_arch = "wasm32")]
fn submit_spawn_spec(spec: SpawnSpecForm, done: impl FnOnce(Result<(), String>) + 'static) {
    use gloo_net::http::Request;
    let body = serde_json::to_string(&spec).unwrap_or_default();
    wasm_bindgen_futures::spawn_local(async move {
        let req = match Request::post("/api/agents")
            .header("Content-Type", "application/json")
            .body(body)
        {
            Ok(r) => r,
            Err(e) => {
                done(Err(format!("request build failed: {e}")));
                return;
            }
        };
        match req.send().await {
            Ok(resp) if resp.status() == 201 => done(Ok(())),
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                done(Err(format!("create failed ({status}): {text}")));
            }
            Err(e) => done(Err(format!("network error: {e}"))),
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn submit_spawn_spec(_spec: SpawnSpecForm, _done: impl FnOnce(Result<(), String>) + 'static) {}
