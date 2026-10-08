terraform {
  required_providers {
    coder = {
      source  = "coder/coder"
      version = "~> 2.0"
    }
    restapi = {
      source  = "Mastercard/restapi"
      version = "3.0.0"
    }
  }
}

variable "provisioner_api_url" {
  type    = string
  default = "http://127.0.0.1:7081"
}
variable "organization_id" {
  type = string
}
variable "project_id" {
  type = string
}
variable "vpc_id" {
  type = string
}
variable "workspace_image" {
  type = string
}
variable "coder_url" {
  type        = string
  description = "Compatibility input; bootstrap uses the live Coder deployment access URL."
}
variable "coder_private_url" {
  type = string
}

provider "restapi" {
  uri                  = var.provisioner_api_url
  write_returns_object = true
  # The local bridge injects and renews task IAM credentials in memory.
  timeout = 60
  retries {
    max_retries = 3
    min_wait    = 1
    max_wait    = 5
  }
}

data "coder_workspace" "me" {}
data "coder_workspace_owner" "me" {}

resource "coder_agent" "main" {
  arch                    = "amd64"
  os                      = "linux"
  connection_timeout      = 600
  startup_script_behavior = "blocking"
  startup_script          = <<-EOT
    #!/bin/sh
    set -eu
    mkdir -p "$HOME/projects"
    mkdir -p "$HOME/.local/bin"
    cat > "$HOME/.local/bin/coder-code-server-supervisor" <<'IDE_SUPERVISOR'
    #!/bin/sh
    set -eu
    umask 077
    cache_dir="$HOME/.cache/coder-code-server"
    mkdir -p "$cache_dir" "$HOME/projects"
    exec 9>"$cache_dir/supervisor.lock"
    flock -n 9 || exit 0
    while :; do
      /opt/code-server/bin/code-server --auth none --bind-addr 127.0.0.1:13337 --disable-telemetry "$HOME/projects" >>"$cache_dir/code-server.log" 2>&1 &
      child=$!
      trap 'kill -TERM "$child" 2>/dev/null || true; wait "$child" 2>/dev/null || true; exit 0' TERM INT
      status=0
      wait "$child" || status=$?
      printf 'code-server exited (status=%s); restarting\n' "$status" >>"$cache_dir/supervisor.log"
      sleep 2
    done
    IDE_SUPERVISOR
    chmod 700 "$HOME/.local/bin/coder-code-server-supervisor"
    nohup "$HOME/.local/bin/coder-code-server-supervisor" >/dev/null 2>&1 </dev/null &
    echo "Standard Develop ready. Persistent files: /root"
  EOT
  env = {
    GIT_AUTHOR_NAME     = coalesce(data.coder_workspace_owner.me.full_name, data.coder_workspace_owner.me.name)
    GIT_AUTHOR_EMAIL    = data.coder_workspace_owner.me.email
    GIT_COMMITTER_NAME  = coalesce(data.coder_workspace_owner.me.full_name, data.coder_workspace_owner.me.name)
    GIT_COMMITTER_EMAIL = data.coder_workspace_owner.me.email
  }
  metadata {
    display_name = "CPU"
    key          = "cpu"
    script       = "coder stat cpu"
    interval     = 10
    timeout      = 1
  }
  metadata {
    display_name = "Memory"
    key          = "memory"
    script       = "coder stat mem"
    interval     = 10
    timeout      = 1
  }
  metadata {
    display_name = "Persistent home"
    key          = "disk"
    script       = "coder stat disk --path /root"
    interval     = 60
    timeout      = 1
  }
}

resource "coder_app" "code_server" {
  agent_id     = coder_agent.main.id
  slug         = "code-server"
  display_name = "VS Code"
  url          = "http://localhost:13337/"
  icon         = "/icon/code.svg"
  subdomain    = false
  share        = "owner"
  order        = 1
  healthcheck {
    url       = "http://localhost:13337/healthz"
    interval  = 5
    threshold = 6
  }
}

module "filebrowser" {
  source        = "registry.coder.com/modules/filebrowser/coder"
  version       = "1.1.6"
  agent_id      = coder_agent.main.id
  agent_name    = "main"
  folder        = "/root"
  database_path = "/root/filebrowser.db"
  subdomain     = false
  order         = 2
}

locals {
  service_name = "coder-ws-${data.coder_workspace.me.id}"
  service_spec = {
    region                = "heteronet-global"
    image                 = var.workspace_image
    replicas              = 1
    stopped               = data.coder_workspace.me.start_count == 0
    cpu_millis            = 4000
    memory_mib            = 8192
    ephemeral_storage_gib = 30
    rootfs_storage_gib    = 3
    ports                 = []
    exposure = {
      type         = "internal"
      traffic_mode = "forwarded"
    }
    egress = {
      mode                    = "internet"
      allow_same_organization = false
    }
    network = {
      vpc_id          = var.vpc_id
      security_groups = ["workspaces"]
      private_name    = local.service_name
    }
    env = {
      HOME              = "/root"
      CODER_AGENT_URL   = var.coder_private_url
      CODER_AGENT_TOKEN = coder_agent.main.token
      PATH              = "/root/.local/bin:/opt/code-server/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    }
    command = ["/bin/bash", "-c"]
    args    = [replace(coder_agent.main.init_script, data.coder_workspace.me.access_url, var.coder_private_url)]
    metadata = {
      application     = "coder"
      coder_workspace = data.coder_workspace.me.id
      coder_owner     = data.coder_workspace_owner.me.name
      coder_owner_id  = data.coder_workspace_owner.me.id
    }
  }
}

# Keep a stable Flash service and its persistent home across stop/start.
# Only deleting the Coder workspace destroys this resource and its data.
resource "restapi_object" "workspace" {
  lifecycle {
    precondition {
      condition     = strcontains(replace(coder_agent.main.init_script, data.coder_workspace.me.access_url, var.coder_private_url), var.coder_private_url)
      error_message = "The agent bootstrap must use the private Coder URL."
    }
  }
  path                    = "/api/v1/organizations/${var.organization_id}/flash/services"
  ignore_server_additions = true
  data = sensitive(jsonencode({
    project_id = var.project_id
    name       = local.service_name
    spec       = local.service_spec
  }))
  update_data = sensitive(jsonencode({
    name = local.service_name
    spec = local.service_spec
  }))
}

resource "coder_metadata" "workspace" {
  resource_id = restapi_object.workspace.id
  item {
    key   = "Flash service"
    value = restapi_object.workspace.id
  }
  item {
    key   = "Resources"
    value = "4 vCPU / 8 GiB RAM / 30 GiB total disk"
  }
  item {
    key   = "Persistent directory"
    value = "/root"
  }
  item {
    key   = "Docker"
    value = "CLI installed; local DinD is unavailable on Flash"
  }
}
