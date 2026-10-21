mock_provider "coder" {
  mock_data "coder_workspace" {
    defaults = {
      id          = "new-workspace"
      name        = "storage-test"
      access_url  = "https://coder.example.test"
      start_count = 1
    }
  }
  mock_data "coder_workspace_owner" {
    defaults = {
      id        = "test-owner"
      name      = "test-owner"
      full_name = "Test Owner"
      email     = "owner@example.test"
    }
  }
  mock_resource "coder_agent" {
    defaults = {
      init_script = "curl http://coder-private:7080"
    }
  }
}

mock_provider "restapi" {}

variables {
  organization_id   = "test-organization"
  project_id        = "test-project"
  vpc_id            = "test-vpc"
  workspace_image   = "example.test/workspace:fixed"
  coder_url         = "https://coder.example.test"
  coder_private_url = "http://coder-private:7080"
}

run "new_workspace_reserves_more_temporary_space" {
  command = apply

  assert {
    condition = (
      jsondecode(restapi_object.workspace.data).spec.rootfs_storage_gib == 10 &&
      jsondecode(restapi_object.workspace.data).spec.ephemeral_storage_gib == 37
    )
    error_message = "New workspaces must reserve 10 GiB for rootfs and keep the original home capacity by adding 7 GiB to total disk."
  }
}

run "existing_workspace_keeps_larger_storage_allocation" {
  command = apply

  override_data {
    target = data.coder_workspace.me
    values = {
      id = "existing-workspace"
    }
  }
  variables {
    workspace_storage_overrides = {
      existing-workspace = {
        total_disk_gib     = 64
        rootfs_storage_gib = 10
      }
    }
  }

  assert {
    condition = (
      jsondecode(restapi_object.workspace.update_data).spec.rootfs_storage_gib == 10 &&
      jsondecode(restapi_object.workspace.update_data).spec.ephemeral_storage_gib == 64
    )
    error_message = "Updating an existing workspace must not shrink a larger disk allocation."
  }
}

run "new_workspace_keeps_default_memory" {
  command = apply

  assert {
    condition     = jsondecode(restapi_object.workspace.data).spec.memory_mib == 8192
    error_message = "Workspaces without an explicit memory allocation must retain the 8 GiB default."
  }
}

run "explicit_persistent_home_survives_workspace_updates" {
  command = apply

  variables {
    workspace_storage_overrides = {
      new-workspace = {
        total_disk_gib         = 57
        rootfs_storage_gib     = 20
        persistent_storage_gib = 36
      }
    }
  }

  assert {
    condition = (
      jsondecode(restapi_object.workspace.update_data).spec.persistent_storage_gib == 36 &&
      jsondecode(restapi_object.workspace.update_data).spec.rootfs_storage_gib == 20 &&
      jsondecode(restapi_object.workspace.update_data).spec.ephemeral_storage_gib == 57 &&
      jsondecode(restapi_object.workspace.update_data).spec.memory_mib == 8192
    )
    error_message = "Explicit persistent storage and the 8 GiB memory allocation must survive a workspace update."
  }
}

run "persistent_home_override_does_not_change_other_workspaces" {
  command = apply

  variables {
    workspace_storage_overrides = {
      another-workspace = {
        total_disk_gib         = 57
        rootfs_storage_gib     = 20
        persistent_storage_gib = 36
      }
    }
  }

  assert {
    condition     = jsondecode(restapi_object.workspace.update_data).spec.persistent_storage_gib == null
    error_message = "Persistent home allocations must only affect the matching workspace ID."
  }
}

run "configured_workspace_keeps_memory_when_running" {
  command = apply

  variables {
    workspace_memory_overrides = { new-workspace = 32768 }
  }

  assert {
    condition     = jsondecode(restapi_object.workspace.update_data).spec.memory_mib == 32768
    error_message = "A workspace rebuild must retain its configured 32 GiB memory allocation."
  }
}

run "configured_workspace_keeps_memory_when_stopped" {
  command = apply

  override_data {
    target = data.coder_workspace.me
    values = {
      id          = "new-workspace"
      start_count = 0
    }
  }

  variables {
    workspace_memory_overrides = { new-workspace = 32768 }
  }

  assert {
    condition = (
      jsondecode(restapi_object.workspace.update_data).spec.memory_mib == 32768 &&
      jsondecode(restapi_object.workspace.update_data).spec.stopped
    )
    error_message = "Stopping a workspace must retain its configured memory for the next start."
  }
}

run "memory_override_does_not_change_other_workspaces" {
  command = apply

  variables {
    workspace_memory_overrides = { another-workspace = 32768 }
  }

  assert {
    condition     = jsondecode(restapi_object.workspace.update_data).spec.memory_mib == 8192
    error_message = "Memory allocations must only affect the matching workspace ID."
  }
}

run "fractional_memory_is_rejected" {
  command = plan

  variables {
    workspace_memory_overrides = { new-workspace = 32768.5 }
  }

  expect_failures = [var.workspace_memory_overrides]
}

run "unsupported_memory_is_rejected" {
  command = plan

  variables {
    workspace_memory_overrides = { new-workspace = 1048577 }
  }

  expect_failures = [var.workspace_memory_overrides]
}
