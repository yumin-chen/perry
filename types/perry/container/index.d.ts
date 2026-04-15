// Type declarations for perry/container — Perry's OCI container management module
// These types are auto-written by `perry init` / `perry types` so IDEs
// and tsc can resolve `import { ... } from "perry/container"`.

// ---------------------------------------------------------------------------
// Container Lifecycle
// ---------------------------------------------------------------------------

/**
 * Configuration for a single container.
 */
export interface ContainerSpec {
  /** Container image (required) */
  image: string;
  /** Container name (optional) */
  name?: string;
  /** Port mappings (e.g., "8080:80") */
  ports?: string[];
  /** Volume mounts (e.g., "/host/path:/container/path:ro") */
  volumes?: string[];
  /** Environment variables */
  env?: Record<string, string>;
  /** Command to run (overrides image CMD) */
  cmd?: string[];
  /** Entrypoint (overrides image ENTRYPOINT) */
  entrypoint?: string[];
  /** Network to attach to */
  network?: string;
  /** Remove container on exit */
  rm?: boolean;
}

/**
 * Handle to a container instance.
 */
export interface ContainerHandle {
  /** Container ID */
  id: string;
  /** Container name (if specified) */
  name?: string;
}

/**
 * Run a container from the given spec.
 * @param spec Container configuration
 * @returns Promise resolving to ContainerHandle
 */
export function run(spec: ContainerSpec): Promise<ContainerHandle>;

/**
 * Create a container from the given spec without starting it.
 * @param spec Container configuration
 * @returns Promise resolving to ContainerHandle
 */
export function create(spec: ContainerSpec): Promise<ContainerHandle>;

/**
 * Start a previously created container.
 * @param id Container ID or name
 * @returns Promise resolving when container is started
 */
export function start(id: string): Promise<void>;

/**
 * Stop a running container.
 * @param id Container ID or name
 * @param timeout Timeout in seconds before force-terminating (default: 10)
 * @returns Promise resolving when container is stopped
 */
export function stop(id: string, timeout?: number): Promise<void>;

/**
 * Remove a container.
 * @param id Container ID or name
 * @param force If true, stop and remove a running container
 * @returns Promise resolving when container is removed
 */
export function remove(id: string, force?: boolean): Promise<void>;

// ---------------------------------------------------------------------------
// Container Inspection and Listing
// ---------------------------------------------------------------------------

/**
 * Information about a container.
 */
export interface ContainerInfo {
  /** Container ID */
  id: string;
  /** Container name */
  name: string;
  /** Image reference */
  image: string;
  /** Container status (e.g., "running", "exited") */
  status: string;
  /** Port mappings */
  ports: string[];
  /** Creation timestamp (ISO 8601) */
  created: string;
}

/**
 * List containers.
 * @param all If true, include stopped containers
 * @returns Promise resolving to array of ContainerInfo
 */
export function list(all?: boolean): Promise<ContainerInfo[]>;

/**
 * Inspect a container.
 * @param id Container ID or name
 * @returns Promise resolving to ContainerInfo
 */
export function inspect(id: string): Promise<ContainerInfo>;

// ---------------------------------------------------------------------------
// Container Logs and Exec
// ---------------------------------------------------------------------------

/**
 * Logs captured from a container.
 */
export interface ContainerLogs {
  /** Standard output */
  stdout: string;
  /** Standard error */
  stderr: string;
}

/**
 * Get logs from a container.
 * @param id Container ID or name
 * @param options Options for logs
 * @returns Promise resolving to ContainerLogs or ReadableStream
 */
export function logs(
  id: string,
  options?: {
    /** If true, return a ReadableStream of log lines */
    follow?: boolean;
    /** Number of lines to return from the end */
    tail?: number;
  }
): Promise<ContainerLogs | ReadableStream<string>>;

/**
 * Execute a command in a running container.
 * @param id Container ID or name
 * @param cmd Command to execute
 * @param options Options for exec
 * @returns Promise resolving to ContainerLogs
 */
export function exec(
  id: string,
  cmd: string[],
  options?: {
    /** Environment variables */
    env?: Record<string, string>;
    /** Working directory */
    workdir?: string;
  }
): Promise<ContainerLogs>;

// ---------------------------------------------------------------------------
// Image Management
// ---------------------------------------------------------------------------

/**
 * Information about a container image.
 */
export interface ImageInfo {
  /** Image ID */
  id: string;
  /** Repository name */
  repository: string;
  /** Image tag */
  tag: string;
  /** Image size in bytes */
  size: number;
  /** Creation timestamp (ISO 8601) */
  created: string;
}

/**
 * Pull a container image from a registry.
 * @param reference Image reference (e.g., "alpine:latest", "cgr.dev/chainguard/alpine-base@sha256:...")
 * @returns Promise resolving when image is pulled
 */
export function pullImage(reference: string): Promise<void>;

/**
 * List images in the local cache.
 * @returns Promise resolving to array of ImageInfo
 */
export function listImages(): Promise<ImageInfo[]>;

/**
 * Remove an image from the local cache.
 * @param reference Image reference
 * @param force If true, remove even if image is in use
 * @returns Promise resolving when image is removed
 */
export function removeImage(reference: string, force?: boolean): Promise<void>;

// ---------------------------------------------------------------------------
// Compose (Multi-Container Orchestration)
// ---------------------------------------------------------------------------

/**
 * Multi-container application specification.
 */
export interface ComposeSpec {
  /** Compose file version */
  version?: string;
  /** Service definitions */
  services: Record<string, ComposeService>;
  /** Network definitions */
  networks?: Record<string, ComposeNetwork>;
  /** Volume definitions */
  volumes?: Record<string, ComposeVolume>;
}

/**
 * Service definition in Compose.
 */
export interface ComposeService {
  /** Container image */
  image: string;
  /** Build configuration */
  build?: {
    /** Build context directory */
    context: string;
    /** Dockerfile path (relative to context) */
    dockerfile?: string;
  };
  /** Command to run */
  command?: string | string[];
  /** Environment variables */
  environment?: Record<string, string> | string[];
  /** Port mappings */
  ports?: string[];
  /** Volume mounts */
  volumes?: string[];
  /** Networks to attach to */
  networks?: string[];
  /** Service dependencies */
  depends_on?: string[];
  /** Restart policy */
  restart?: string;
  /** Healthcheck configuration */
  healthcheck?: ComposeHealthcheck;
}

/**
 * Healthcheck configuration.
 */
export interface ComposeHealthcheck {
  /** Test command (string or array) */
  test: string | string[];
  /** Check interval (e.g., "30s") */
  interval?: string;
  /** Timeout (e.g., "10s") */
  timeout?: string;
  /** Number of retries before unhealthy */
  retries?: number;
  /** Startup grace period (e.g., "40s") */
  start_period?: string;
}

/**
 * Network configuration.
 */
export interface ComposeNetwork {
  /** Network driver */
  driver?: string;
  /** External network reference */
  external?: boolean;
  /** Network name */
  name?: string;
}

/**
 * Volume configuration.
 */
export interface ComposeVolume {
  /** Volume driver */
  driver?: string;
  /** External volume reference */
  external?: boolean;
  /** Volume name */
  name?: string;
}

/**
 * Bring up a Compose stack.
 * @param spec Compose specification
 * @returns Promise resolving to the stack ID (number)
 */
export function composeUp(spec: ComposeSpec): Promise<number>;

// ---------------------------------------------------------------------------
// Platform Information
// ---------------------------------------------------------------------------

/**
 * Get the name of the container backend being used.
 * @returns "apple/container" on macOS/iOS, "podman" on all other platforms
 */
export function getBackend(): string;

/**
 * Probe for available container runtimes and return details about each.
 * @returns Promise resolving to a JSON array of backend probe results
 */
export function detectBackend(): Promise<string>;
