{{- define "heterocloud.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "heterocloud.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name (include "heterocloud.name" .) | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}

{{- define "heterocloud.labels" -}}
app.kubernetes.io/name: {{ include "heterocloud.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | quote }}
{{- end }}

{{- define "heterocloud.selectorLabels" -}}
app.kubernetes.io/name: {{ include "heterocloud.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: api
{{- end }}

