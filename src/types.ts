/** Mirrors `Config` in src-tauri/src/config.rs — keep the two in step. */
export interface Config {
  install_dir: string;
  launch_args: string;
  /** Last `ip:port` entered in the connect prompt; pre-fills it next launch. */
  last_server: string;
  hosts_redirect: boolean;
  close_on_launch: boolean;
  auto_update: boolean;
  verify_before_launch: boolean;
  debug_logging: boolean;
  client_fixes_enabled: boolean;

  /** DPAPI-encrypted launcher key. Never the key itself. */
  auth_key_sealed: string;
  /** Identifies this installation; a label, not a secret. */
  device_id: string;
  account_id: string;
  display_name: string;
  key_status: string;
}

/** Mirrors `auth::AuthStatus` in src-tauri/src/auth.rs — keep the two in step. */
export interface AuthStatus {
  signed_in: boolean;
  account_id: string;
  display_name: string;
  /** "active", "suspended" or "revoked" as the backend last reported it. */
  status: string;
}

export interface InstallState {
  installed: boolean;
  exe_path: string | null;
}

export interface HostsStatus {
  path: string;
  /** False means the launcher is not running as administrator. */
  writable: boolean;
  applied: boolean;
  /** Hand-written lines mapping the same hostnames elsewhere in the file. */
  conflicts: string[];
}

export type Tab = "play" | "download" | "settings";

/** Mirrors `download::Status` in src-tauri/src/download.rs — keep the two in step. */
export interface DownloadStatus {
  phase: "idle" | "checking" | "downloading" | "paused" | "verifying" | "extracting" | "done" | "failed";
  dir: string;
  /** Bytes done in the current step (downloaded / hashed / unpacked). */
  done: number;
  /** Size of the current step; 0 while unknown. */
  total: number;
  /** Bytes per second, smoothed. */
  speed: number;
  eta_secs: number | null;
  message: string;
  free_bytes: number | null;
  needed_bytes: number | null;
  retries: number;
  install_dir: string;
}

export type Phase = "ready" | "not-installed";

/** Mirrors `news::NewsItem` in src-tauri/src/news.rs — keep the two in step. */
export interface NewsItem {
  id: string;
  tag: string;
  title: string;
  description: string;
  /** URL of the slide's background image. */
  image: string;
  /** ISO-8601. Empty means "already started". */
  starts_at: string;
  /** ISO-8601. Empty means "never ends". */
  ends_at: string;
  clickable: boolean;
  /** Only meaningful when `clickable` is true. */
  url?: string | null;
}
