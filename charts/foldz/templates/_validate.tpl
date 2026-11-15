{{/*
Cross-field rules values.schema.json cannot state with a readable message.
Each one fails rendering rather than render a pod the binary refuses or a
setting that has no effect.
*/}}
{{- define "foldz.validate" -}}
{{- $v := .Values }}

{{- if ne (int $v.replicas) 1 }}
{{- fail (printf "replicas: %v is not supported; foldz does not implement replication yet, so the chart runs exactly 1 audio replica" $v.replicas) }}
{{- end }}
{{- if $v.witness.enabled }}
{{- fail "witness.enabled: foldz does not implement replication yet, so there is no witness to run" }}
{{- end }}

{{- if $v.nmos.enabled }}
{{- fail "nmos.enabled: foldz does not implement NMOS yet" }}
{{- end }}

{{- if eq $v.auth.mode "none" }}
{{- if $v.ingress.enabled }}
{{- fail "auth.mode none: refused with ingress.enabled; set auth.mode token or proxy" }}
{{- end }}
{{- if ne $v.service.type "ClusterIP" }}
{{- fail (printf "auth.mode none: refused with service.type %s; only ClusterIP keeps the unauthenticated API in the cluster" $v.service.type) }}
{{- end }}
{{- end }}
{{- if and (eq $v.auth.mode "token") (not $v.auth.token.existingSecret) }}
{{- fail "auth.token.existingSecret: required with auth.mode token" }}
{{- end }}
{{- if eq $v.auth.mode "proxy" }}
{{- if not $v.auth.proxy.sidecar.image.repository }}
{{- fail "auth.proxy.sidecar.image.repository: required with auth.mode proxy" }}
{{- end }}
{{- if include "foldz.tlsOn" (dict "ctx" . "listener" "api") }}
{{- fail "tls.listeners: api is refused with auth.mode proxy; the API listener binds 127.0.0.1 behind the sidecar, so terminate TLS at the sidecar or in front of it" }}
{{- end }}
{{- if not $v.auth.proxy.groupRoles }}
{{- fail "auth.proxy.groupRoles: required with auth.mode proxy; without it nobody gets a role" }}
{{- end }}
{{- end }}

{{- if eq $v.tls.mode "existingSecret" }}
{{- if not $v.tls.existingSecret }}
{{- fail "tls.existingSecret: required with tls.mode existingSecret" }}
{{- end }}
{{- else if eq $v.tls.mode "certManager" }}
{{- if not $v.tls.certManager.issuerRef.name }}
{{- fail "tls.certManager.issuerRef.name: required with tls.mode certManager" }}
{{- end }}
{{- if not (.Capabilities.APIVersions.Has "cert-manager.io/v1/Certificate") }}
{{- fail "tls.mode certManager: the cluster serves no cert-manager.io/v1 Certificate; install cert-manager or use tls.mode existingSecret" }}
{{- end }}
{{- end }}
{{- if and (ne $v.tls.mode "none") (not $v.tls.listeners) }}
{{- fail (printf "tls.listeners: empty with tls.mode %s; name api, nmos or both" $v.tls.mode) }}
{{- end }}

{{- if and $v.ingress.enabled (not $v.ingress.hosts) }}
{{- fail "ingress.hosts: at least one host is required with ingress.enabled" }}
{{- end }}

{{- if not $v.mxl.domains }}
{{- fail "mxl.domains: at least one domain is required" }}
{{- end }}
{{- $names := list }}
{{- range $i, $d := $v.mxl.domains }}
{{- if has $d.name $names }}
{{- fail (printf "mxl.domains[%d]: name %q is used twice" $i $d.name) }}
{{- end }}
{{- $names = append $names $d.name }}
{{- if and $d.hostPath $d.volume }}
{{- fail (printf "mxl.domains[%d]: hostPath and volume are mutually exclusive" $i) }}
{{- end }}
{{- if not (or $d.hostPath $d.volume) }}
{{- fail (printf "mxl.domains[%d]: hostPath or volume is required" $i) }}
{{- end }}
{{- if and $d.volume (hasKey $d.volume "hostPath") }}
{{- fail (printf "mxl.domains[%d].volume: use hostPath on the domain, not inside volume" $i) }}
{{- end }}
{{- end }}

{{- if not $v.config.inputs }}
{{- fail "config.inputs: at least one input is required" }}
{{- end }}
{{- if not $v.config.outputs }}
{{- fail "config.outputs: at least one output is required" }}
{{- end }}
{{- range $i, $o := $v.config.outputs }}
{{- if ne (len ($o.flows | default list)) (int $v.replicas) }}
{{- fail (printf "config.outputs[%d].flows: has %d flow ids for %d replica(s); each replica writes the member flow at its ordinal" $i (len ($o.flows | default list)) (int $v.replicas)) }}
{{- end }}
{{- end }}

{{- if and $v.rt.enabled (eq (toString $v.rt.rtCpu) "auto") }}
{{- $req := dig "requests" "cpu" "" $v.resources }}
{{- $lim := dig "limits" "cpu" "" $v.resources }}
{{- if or (not $req) (not $lim) }}
{{- fail "resources: rt.rtCpu auto needs resources.requests.cpu and resources.limits.cpu; set both, or rt.rtCpu none" }}
{{- end }}
{{- $r := include "foldz.millicores" $req | int }}
{{- $l := include "foldz.millicores" $lim | int }}
{{- if or (ne $r $l) (ne (mod $r 1000) 0) (eq $r 0) }}
{{- fail (printf "resources: rt.rtCpu auto needs the CPU request to equal the limit in whole CPUs (request %v, limit %v); the RT thread is pinned to an exclusive CPU only then" $req $lim) }}
{{- end }}
{{- $rmem := dig "requests" "memory" "" $v.resources }}
{{- $lmem := dig "limits" "memory" "" $v.resources }}
{{- if or (not $rmem) (ne (toString $rmem) (toString $lmem)) }}
{{- fail (printf "resources: rt.rtCpu auto needs resources.requests.memory equal to resources.limits.memory (request %v, limit %v); Guaranteed QoS, which exclusive CPUs need, requires both" (default "unset" $rmem) (default "unset" $lmem)) }}
{{- end }}
{{- if eq $v.auth.mode "proxy" }}
{{- $sc := $v.auth.proxy.sidecar.resources }}
{{- $sreq := dig "requests" dict $sc }}
{{- $slim := dig "limits" dict $sc }}
{{- range $k := list "cpu" "memory" }}
{{- if or (not (hasKey $sreq $k)) (ne (toString (get $sreq $k)) (toString (get $slim $k))) }}
{{- fail (printf "auth.proxy.sidecar.resources: rt.rtCpu auto with auth.mode proxy needs the sidecar's %s request to equal its limit; a pod is Guaranteed only when every container is" $k) }}
{{- end }}
{{- end }}
{{- end }}
{{- end }}
{{- end }}
