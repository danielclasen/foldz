{{- define "foldz.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "foldz.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else if contains (include "foldz.name" .) .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name (include "foldz.name" .) | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}

{{/* Service names get a suffix, so the base name leaves room for it. */}}
{{- define "foldz.base" -}}
{{- include "foldz.fullname" . | trunc 52 | trimSuffix "-" }}
{{- end }}

{{- define "foldz.nmosService" -}}{{ include "foldz.base" . }}-nmos{{- end }}
{{- define "foldz.opsService" -}}{{ include "foldz.base" . }}-ops{{- end }}
{{- define "foldz.headlessService" -}}{{ include "foldz.base" . }}-headless{{- end }}
{{- define "foldz.tlsSecret" -}}
{{- if eq .Values.tls.mode "existingSecret" }}{{ .Values.tls.existingSecret }}{{ else }}{{ include "foldz.base" . }}-tls{{ end }}
{{- end }}

{{- define "foldz.selectorLabels" -}}
app.kubernetes.io/name: {{ include "foldz.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: audio
{{- end }}

{{- define "foldz.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{ include "foldz.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "foldz.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "foldz.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "foldz.image" -}}
{{- if .Values.image.digest }}
{{- printf "%s@%s" .Values.image.repository .Values.image.digest }}
{{- else }}
{{- printf "%s:%s" .Values.image.repository (default .Chart.AppVersion .Values.image.tag) }}
{{- end }}
{{- end }}

{{- define "foldz.sidecarImage" -}}
{{- $i := .Values.auth.proxy.sidecar.image }}
{{- if and $i.tag $i.digest }}
{{- printf "%s:%s@%s" $i.repository $i.tag $i.digest }}
{{- else if $i.digest }}
{{- printf "%s@%s" $i.repository $i.digest }}
{{- else }}
{{- printf "%s:%s" $i.repository $i.tag }}
{{- end }}
{{- end }}

{{/* The names an in-cluster client reaches a Service by. */}}
{{- define "foldz.serviceNames" -}}
{{- $ns := .ctx.Release.Namespace }}
{{- list .name (printf "%s.%s" .name $ns) (printf "%s.%s.svc" .name $ns) | toJson }}
{{- end }}

{{- define "foldz.tlsOn" -}}
{{- if and (ne .ctx.Values.tls.mode "none") (has .listener .ctx.Values.tls.listeners) }}true{{ end }}
{{- end }}

{{- define "foldz.millicores" -}}
{{- $q := toString . }}
{{- if hasSuffix "m" $q }}{{ trimSuffix "m" $q | int }}{{ else }}{{ mulf (float64 $q) 1000 | int }}{{ end }}
{{- end }}

{{/*
The config file the binary reads with --config. Secrets appear as paths of
mounted files, never as values.
*/}}
{{- define "foldz.config" -}}
{{- $v := .Values }}
{{- $tlsFiles := dict "mode" "files" "certFile" "/var/run/secrets/foldz/tls/tls.crt" "keyFile" "/var/run/secrets/foldz/tls/tls.key" }}
{{- $proxy := eq $v.auth.mode "proxy" }}
{{- $api := dict "bind" (ternary "127.0.0.1" "0.0.0.0" $proxy) "port" (int $v.service.port) }}
{{- if include "foldz.tlsOn" (dict "ctx" . "listener" "api") }}
{{- $_ := set $api "tls" $tlsFiles }}
{{- end }}
{{- $nmosListener := dict "bind" "0.0.0.0" "port" (int $v.nmos.port) }}
{{- if include "foldz.tlsOn" (dict "ctx" . "listener" "nmos") }}
{{- $_ := set $nmosListener "tls" $tlsFiles }}
{{- end }}
{{- $listeners := dict "api" $api "nmos" $nmosListener "ops" (dict "bind" "0.0.0.0" "port" (int $v.ops.port)) }}

{{- $auth := dict "mode" $v.auth.mode }}
{{- if eq $v.auth.mode "token" }}
{{- $_ := set $auth "token" (dict "file" (printf "/var/run/secrets/foldz/token/%s" $v.auth.token.key)) }}
{{- else if $proxy }}
{{- $_ := set $auth "proxy" (dict "userHeader" $v.auth.proxy.userHeader "groupsHeader" $v.auth.proxy.groupsHeader "groupRoles" $v.auth.proxy.groupRoles) }}
{{- end }}

{{- $hosts := concat (include "foldz.serviceNames" (dict "ctx" . "name" (include "foldz.fullname" .)) | fromJsonArray) $v.api.allowedHosts }}
{{- $origins := list }}
{{- if $v.ingress.enabled }}
{{- $tlsHosts := list }}
{{- range $v.ingress.tls }}{{ $tlsHosts = concat $tlsHosts (.hosts | default list) }}{{ end }}
{{- range $v.ingress.hosts }}
{{- $hosts = append $hosts . }}
{{- $origins = append $origins (printf "%s://%s" (ternary "https" "http" (has . $tlsHosts)) .) }}
{{- end }}
{{- end }}
{{- $origins = concat $origins $v.api.allowedOrigins }}

{{- $nmos := dict "enabled" $v.nmos.enabled }}

{{- $domains := list }}
{{- range $v.mxl.domains }}
{{- $domains = append $domains (dict "name" .name "path" (printf "/mxl/%s" .name)) }}
{{- end }}

{{- $config := dict
  "engine" $v.engine
  "rt" $v.rt
  "mxl" (dict "libraryPath" $v.mxl.libraryPath)
  "domains" $domains
  "nmos" $nmos
  "listeners" $listeners
  "auth" $auth
  "api" (dict "allowedHosts" ($hosts | uniq) "allowedOrigins" ($origins | uniq))
}}
{{- toPrettyJson (merge $config (deepCopy $v.config)) }}
{{- end }}
