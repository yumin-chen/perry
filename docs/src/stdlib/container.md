# Containers

The `perry/container` and `perry/compose` modules provide high-level APIs for managing OCI containers and multi-container stacks directly from Perry applications.

## Prerequisites

Perry automatically detects and uses the best available container runtime on your system. The following runtimes are supported:

| Platform | Supported Backends (in priority order) |
|---|---|
| **macOS / iOS** | `apple/container` → `orbstack` → `colima` → `rancher-desktop` → `lima` → `podman` → `docker` |
| **Linux** | `podman` → `nerdctl` → `docker` |
| **Windows** | `podman` → `docker` |

If no container runtime is found, Perry will offer to install one for you during the first use (unless `PERRY_NO_INSTALL_PROMPT=1` is set).

## Container Lifecycle (`perry/container`)

Use the `perry/container` module to run and manage individual containers.

### Running a Container

```typescript
import { run } from "perry/container";

const container = await run({
  image: "alpine",
  cmd: ["echo", "hello from perry"],
  rm: true,
  // Security options
  user: "nobody",
  workdir: "/app",
  cap_drop: ["ALL"],
});

console.log(`Started container: ${container.id}`);
```

### Managing Containers

```typescript
import { list, stop, remove, inspect } from "perry/container";

// List all running containers
const containers = await list();

// Stop a container
await stop("my-container-id", 10);

// Remove a container
await remove("my-container-id", true);

// Get container details
const info = await inspect("my-container-id");
console.log(info.status);
```

### Logs and Exec

```typescript
import { logs, exec } from "perry/container";

// Fetch logs
const output = await logs("my-container-id", { tail: 100 });
console.log(output.stdout);

// Run a command in a running container
const result = await exec("my-container-id", ["ls", "-la"]);
console.log(result.stdout);
```

## Compose Orchestration (`perry/compose`)

The `perry/compose` module provides a Docker Compose-like experience for managing multi-container applications using TypeScript object literals.

### Bringing Up a Stack

```typescript
import { up } from "perry/compose";

const handle = await up({
  name: "my-app",
  services: {
    web: {
      image: "nginx:alpine",
      ports: ["8080:80"],
      // Security: run as non-root with dropped capabilities
      user: "nginx",
      cap_drop: ["ALL"],
    },
    db: {
      image: "postgres:15",
      environment: {
        POSTGRES_PASSWORD: "password"
      },
      // Security: read-only root filesystem
      read_only: true,
      volumes: ["pgdata:/var/lib/postgresql/data"],
    }
  }
});

console.log(`Stack is up! ID: ${handle}`);
```

### Stack Management

```typescript
import { down, ps, config } from "perry/compose";

// Get status of services in the stack
const statuses = await ps(handle);

// Get the resolved YAML configuration
const yaml = await config(handle);

// Tear down the stack and its networks
await down(handle, { volumes: true });
```

## Security and Sandboxing

Perry implements several security measures when running containers:

- **Idempotency**: `up()` skips services that are already running with the same configuration.
- **Dependency Order**: Services are started in the order specified by `depends_on` using Kahn's algorithm.
- **Rollback**: If any part of the orchestration fails, Perry automatically rolls back and cleans up all resources created during that session.
- **Verification**: Images can be verified using `cosign` signatures before being pulled.
- **Capability Isolation**: Internal capability checks run in strictly sandboxed containers with no network (by default), read-only roots, and dropped capabilities.

### Security Options

Both `ContainerSpec` and `ComposeService` support the following security options:

| Option | Type | Description |
|--------|------|-------------|
| `privileged` | `boolean` | Run container in privileged mode (grants all capabilities) |
| `user` | `string` | Username or UID:GID to run as |
| `workdir` | `string` | Working directory inside the container |
| `cap_add` | `string[]` | Add Linux capabilities (e.g., `["NET_ADMIN"]`) |
| `cap_drop` | `string[]` | Drop Linux capabilities (e.g., `["ALL"]`) |
| `read_only` | `boolean` | Mount container root filesystem as read-only |
| `seccomp` | `string` | Path to seccomp profile file (e.g., `"/etc/seccomp/profile.json"`)

### Recommended Security Patterns

```typescript
// Untrusted workload: minimal privileges
{
  image: "untrusted-code",
  user: "nobody",
  cap_drop: ["ALL"],
  read_only: true,
  // No network access (not attached to any network)
}

// Privileged workload (use with caution)
{
  image: "system-tools",
  privileged: true,
  // Only when absolutely necessary
}

// Network-capable service
{
  image: "nginx",
  cap_add: ["NET_BIND_SERVICE"],
  user: "nginx",
}

// Custom seccomp profile
{
  image: "alpine",
  seccomp: "/etc/seccomp/custom-profile.json",
  cap_drop: ["ALL"],
}
```

## Environment Variables

- `PERRY_CONTAINER_BACKEND`: Override the auto-detection and force a specific backend (e.g., `podman`).
- `PERRY_NO_INSTALL_PROMPT`: Disable the interactive installer prompt if no backend is found.
