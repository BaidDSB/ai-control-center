use super::schema::{EnginePreset, ModelProfile, RoutingSet, StackConfig};
use std::collections::{HashMap, HashSet};
use std::path::Path;

impl ModelProfile {
    fn ensure_swap_port_in_args(args: &mut Vec<String>, host: &str) {
        if args.is_empty() || args.iter().any(|a| a.contains("${PORT}")) {
            return;
        }
        let mut patched = false;
        let mut i = 0;
        while i < args.len() {
            let tok = args[i].as_str();
            if matches!(tok, "--port" | "-p") && i + 1 < args.len() {
                args[i + 1] = "${PORT}".to_string();
                patched = true;
                break;
            }
            if let Some(prefix) = ["--port=", "-p="].iter().find(|p| tok.starts_with(*p)) {
                args[i] = format!("{prefix}${{PORT}}");
                patched = true;
                break;
            }
            if matches!(tok, "--addr" | "--listen" | "--bind") && i + 1 < args.len() {
                let h = args[i + 1].rsplit_once(':').map(|(a, _)| a).unwrap_or(host);
                args[i + 1] = format!("{h}:${{PORT}}");
                patched = true;
                break;
            }
            if let Some(prefix) = ["--addr=", "--listen=", "--bind="].iter().find(|p| tok.starts_with(*p)) {
                let val = &tok[prefix.len()..];
                let h = val.rsplit_once(':').map(|(a, _)| a).unwrap_or(host);
                args[i] = format!("{prefix}{h}:${{PORT}}");
                patched = true;
                break;
            }
            i += 1;
        }
        if !patched {
            let h = if host.trim().is_empty() { "127.0.0.1" } else { host.trim() };
            if args.first().map(|s| s.eq_ignore_ascii_case("serve")).unwrap_or(false) {
                args.push("--addr".to_string());
                args.push(format!("{h}:${{PORT}}"));
            } else {
                args.push("--port".to_string());
                args.push("${PORT}".to_string());
            }
        }
    }

    pub fn build_swap_cmd_and_env_with_engines(
        &self,
        models_dir: &str,
        global_llama_dir: &str,
        saved_engines: &[EnginePreset],
    ) -> (String, Vec<String>) {
        self.build_swap_cmd_and_env_with_engines_and_rpc(
            models_dir,
            global_llama_dir,
            saved_engines,
            self.single.legacy_rpc_opt(),
        )
    }

    pub fn build_swap_cmd_and_env_with_engines_and_rpc(
        &self,
        models_dir: &str,
        global_llama_dir: &str,
        saved_engines: &[EnginePreset],
        resolved_rpc: Option<&str>,
    ) -> (String, Vec<String>) {
        let exe = Path::new(self.effective_engine_dir(global_llama_dir)).join(self.effective_engine_exe());
        let exe_s = quote_for_swap(&exe.to_string_lossy());
        let mut args = self.build_args_with_port_and_rpc(models_dir, "${PORT}", resolved_rpc);

        if args.is_empty() && self.effective_engine_kind() == "custom" {
            let lbl = self.engine_label.trim();
            let ex = self.effective_engine_exe();
            if let Some(eng) = saved_engines.iter().find(|e| {
                (!lbl.is_empty() && e.label.eq_ignore_ascii_case(lbl)) || e.exe_name.eq_ignore_ascii_case(ex)
            })
                && !eng.default_args.trim().is_empty() {
                    args = self.expand_custom_template(&eng.default_args, models_dir, "${PORT}");
                }
        }
        Self::ensure_swap_port_in_args(&mut args, &self.host);

        let mut env_vars = Vec::new();
        if self.should_disable_cuda_pinned() {
            env_vars.push("GGML_CUDA_NO_PINNED=1".to_string());
        }
        let mut parts = Vec::with_capacity(args.len() + 1);
        parts.push(exe_s);
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--chat-template-kwargs" && i + 1 < args.len() {
                env_vars.push(format!("LLAMA_ARG_CHAT_TEMPLATE_KWARGS={}", args[i + 1]));
                i += 2;
            } else {
                parts.push(quote_for_swap(&args[i]));
                i += 1;
            }
        }
        (parts.join(" "), env_vars)
    }

    pub fn build_swap_cmd_and_env(&self, models_dir: &str, global_llama_dir: &str) -> (String, Vec<String>) {
        let exe = Path::new(self.effective_engine_dir(global_llama_dir)).join(self.effective_engine_exe());
        let exe_s = quote_for_swap(&exe.to_string_lossy());
        let args = self.build_args_with_port(models_dir, "${PORT}");

        let mut env_vars = Vec::new();
        if self.should_disable_cuda_pinned() {
            env_vars.push("GGML_CUDA_NO_PINNED=1".to_string());
        }
        let mut parts = Vec::with_capacity(args.len() + 1);
        parts.push(exe_s);
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--chat-template-kwargs" && i + 1 < args.len() {
                env_vars.push(format!("LLAMA_ARG_CHAT_TEMPLATE_KWARGS={}", args[i + 1]));
                i += 2;
            } else {
                parts.push(quote_for_swap(&args[i]));
                i += 1;
            }
        }
        (parts.join(" "), env_vars)
    }
}

pub(crate) fn quote_for_swap(tok: &str) -> String {
    if tok.contains('"') || tok.contains(' ') {
        format!("\"{}\"", tok.replace('"', "\\\""))
    } else {
        tok.to_string()
    }
} impl StackConfig {
    pub fn build_routing_set_yaml(&self, set: &RoutingSet) -> String {
        self.build_routing_set_yaml_with_rpc_status(set, None)
    }

    pub fn build_routing_set_yaml_with_rpc_status(
        &self,
        set: &RoutingSet,
        rpc_reachable: Option<bool>,
    ) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Auto-generated by AI Control Center v{}\n", env!("CARGO_PKG_VERSION")));
        out.push_str(&format!("# Routing Set: {}\n", set.name));
        out.push_str("healthCheckTimeout: 180\nlogToStdout: \"both\"\n\nmodels:\n");

        let mut used_names: HashSet<String> = HashSet::new();
        let mut pid_to_key: HashMap<String, String> = HashMap::new();
        for pid in &set.profile_ids {
            let Some(prof) = self.find_profile(pid) else { continue; };
            let mut key = prof.swap_model_id();
            if used_names.contains(&key) {
                key = prof.id.clone();
            }
            used_names.insert(key.clone());
            pid_to_key.insert(prof.id.clone(), key.clone());

            let resolved_rpc = self.effective_rpc_for_profile(&prof.single, rpc_reachable);
            let (cmd, env_vars) = prof.build_swap_cmd_and_env_with_engines_and_rpc(
                &self.models_dir,
                &self.llama_dir,
                &self.saved_engines,
                resolved_rpc.as_deref(),
            );
            out.push_str(&format!("  \"{}\":\n", key));
            if prof.ttl_seconds > 0 {
                out.push_str(&format!("    ttl: {}\n", prof.ttl_seconds));
            }
            let check_ep = prof.check_endpoint.trim();
            if !check_ep.is_empty() {
                let safe_ep = check_ep.replace('"', "\\\"");
                out.push_str(&format!("    checkEndpoint: \"{safe_ep}\"\n"));
            }
            if !env_vars.is_empty() {
                out.push_str("    env:\n");
                for ev in env_vars {
                    let safe = ev.replace('\'', "''");
                    out.push_str(&format!("      - '{safe}'\n"));
                }
            }
            out.push_str("    cmd: |\n");
            out.push_str(&format!("      {}\n\n", cmd));
        }

        // Optional concurrent groups (llama-swap `groups:`)
        let mut emitted_groups = false;
        for grp in &set.concurrent_groups {
            let gname = grp.name.trim();
            if gname.is_empty() {
                continue;
            }
            let mut member_keys: Vec<String> = Vec::new();
            for m in &grp.members {
                let resolved = pid_to_key
                    .get(m)
                    .cloned()
                    .or_else(|| if used_names.contains(m) { Some(m.clone()) } else { None });
                if let Some(k) = resolved
                    && !member_keys.contains(&k) {
                        member_keys.push(k);
                    }
            }
            if member_keys.is_empty() {
                continue;
            }
            if !emitted_groups {
                out.push_str("groups:\n");
                emitted_groups = true;
            }
            let safe_gn = gname.replace('"', "\\\"");
            out.push_str(&format!("  \"{safe_gn}\":\n"));
            out.push_str(&format!("    swap: {}\n", grp.swap));
            out.push_str(&format!("    exclusive: {}\n", grp.exclusive));
            out.push_str(&format!("    persistent: {}\n", grp.persistent));
            out.push_str("    members:\n");
            for mk in member_keys {
                let safe_mk = mk.replace('"', "\\\"");
                out.push_str(&format!("      - \"{safe_mk}\"\n"));
            }
            out.push('\n');
        }

        // Optional remote peers (llama-swap `peers:`)
        let mut emitted_peers = false;
        for peer in &set.peers {
            if !peer.enabled {
                continue;
            }
            let pname = peer.name.trim();
            let proxy = peer.proxy.trim();
            if pname.is_empty() || proxy.is_empty() {
                continue;
            }
            if !emitted_peers {
                out.push_str("peers:\n");
                emitted_peers = true;
            }
            let safe_pn = pname.replace('"', "\\\"");
            let safe_px = proxy.replace('"', "\\\"");
            out.push_str(&format!("  \"{safe_pn}\":\n"));
            out.push_str(&format!("    proxy: \"{safe_px}\"\n"));
            let ak = peer.api_key.trim();
            if !ak.is_empty() {
                let safe_ak = ak.replace('"', "\\\"");
                out.push_str(&format!("    apiKey: \"{safe_ak}\"\n"));
            }
            let models: Vec<&str> = peer
                .models
                .iter()
                .map(|m| m.trim())
                .filter(|m| !m.is_empty())
                .collect();
            if !models.is_empty() {
                out.push_str("    models:\n");
                for m in models {
                    let safe_m = m.replace('"', "\\\"");
                    out.push_str(&format!("      - \"{safe_m}\"\n"));
                }
            }
            out.push('\n');
        }
        out
    }

    pub fn build_routing_set_ini(&self, set: &RoutingSet) -> String {
        self.build_routing_set_ini_with_rpc_status(set, None)
    }

    pub fn build_routing_set_ini_with_rpc_status(
        &self,
        set: &RoutingSet,
        rpc_reachable: Option<bool>,
    ) -> String {
        let mut out = String::from("[*]\nflash-attn = on\nthreads = 6\nparallel = 1\njinja = true\n\n");
        for pid in &set.profile_ids {
            if let Some(prof) = self.find_profile(pid)
                && prof.effective_engine_kind() != "custom" {
                    let resolved_rpc = self.effective_rpc_for_profile(&prof.single, rpc_reachable);
                    if (prof.use_manual_flags && !prof.manual_flags.trim().is_empty()) || prof.use_advanced_flags {
                        let sec_name = prof.swap_model_id();
                        out.push_str(&format!("[{sec_name}]\n"));
                        let toks = prof.build_args_with_port_and_rpc(&self.models_dir, "8080", resolved_rpc.as_deref());
                        let mut i = 0;
                        while i < toks.len() {
                            let flag = toks[i].as_str();
                            if flag == "--host" || flag == "--port" || flag == "--alias" {
                                i += 2;
                                continue;
                            }
                            if flag == "-m" && i + 1 < toks.len() {
                                out.push_str(&format!("model = {}\n", toks[i + 1]));
                                i += 2;
                            } else if flag.starts_with('-') {
                                let k = flag.trim_start_matches('-');
                                if i + 1 < toks.len() && !toks[i + 1].starts_with('-') {
                                    out.push_str(&format!("{k} = {}\n", toks[i + 1]));
                                    i += 2;
                                } else {
                                    out.push_str(&format!("{k} = true\n"));
                                    i += 1;
                                }
                            } else {
                                i += 1;
                            }
                        }
                        out.push('\n');
                    } else {
                        out.push_str(&prof.single.build_ini_section_with_rpc(&self.models_dir, &prof.id, resolved_rpc.as_deref()));
                        out.push_str("\n\n");
                    }
                }
        }
        out
    }
}
