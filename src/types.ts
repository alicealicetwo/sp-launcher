/** Mirrors `Config` in src-tauri/src/config.rs — keep the two in step. */
export interface Config {
  install_dir: string;
  manifest_url: string;
  /** URL the launcher fetches the news feed (`NewsItem[]` JSON) from. */
  news_url: string;
  launch_args: string;
  hosts_redirect: boolean;
  backend_ip: string;
  hosts_domains: string[];
  close_on_launch: boolean;
  auto_update: boolean;
  verify_before_launch: boolean;
  debug_logging: boolean;
  /** Files transferred at once, 1-16. */
  download_threads: number;
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

export type Tab = "play" | "settings";

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
