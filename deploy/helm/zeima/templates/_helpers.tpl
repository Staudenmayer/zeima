{{- define "zeima.name" -}}{{ .Release.Name }}{{- end -}}

{{- define "zeima.labels" -}}
app.kubernetes.io/part-of: zeima
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}

{{- define "zeima.url.host" -}}{{ . | trimPrefix "https://" | trimPrefix "http://" | trimSuffix "/" }}{{- end -}}

{{- define "zeima.secretName" -}}
{{- if .Values.secrets.existingSecret }}{{ .Values.secrets.existingSecret }}{{ else }}{{ .Release.Name }}-secrets{{ end -}}
{{- end -}}

{{- define "zeima.dbHost" -}}
{{- if .Values.postgres.enabled }}{{ .Release.Name }}-postgres{{ else }}{{ required "externalDatabase.host is required when postgres.enabled is false" .Values.externalDatabase.host }}{{ end -}}
{{- end -}}

{{- define "zeima.apiImage" -}}
{{ .Values.api.image.repository }}:{{ .Values.api.image.tag | default .Chart.AppVersion }}
{{- end -}}

{{- /* One secret value: what was given, else what the last release had, else a new random one. */ -}}
{{- define "zeima.secretValue" -}}
{{- $root := index . 0 -}}
{{- $key := index . 1 -}}
{{- $given := index . 2 -}}
{{- $existing := lookup "v1" "Secret" $root.Release.Namespace (printf "%s-secrets" $root.Release.Name) -}}
{{- if $given }}{{ $given }}
{{- else if and $existing (hasKey $existing.data $key) }}{{ index $existing.data $key | b64dec }}
{{- else }}{{ randAlphaNum 40 }}{{ end -}}
{{- end -}}
