{{- define "heterocloud.databaseTlsEnv" -}}
{{- if .Values.databaseTls.caSecretName }}
- name: PGSSLROOTCERT
  valueFrom:
    secretKeyRef:
      name: {{ .Values.databaseTls.caSecretName | quote }}
      key: {{ required "databaseTls.caSecretKey is required" .Values.databaseTls.caSecretKey | quote }}
      optional: false
{{- end }}
{{- end }}
