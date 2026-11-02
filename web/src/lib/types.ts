export type Phase =
  | 'queued'
  | 'sending'
  | 'accepted_by_cloud'
  | 'awaiting_device'
  | 'confirmed'
  | 'target_matched'
  | 'rejected'
  | 'transport_failed'
  | 'timed_out'
  | 'unknown';
export interface Command {
  id: number;
  target: number;
  phase: Phase;
  server_accepted?: boolean;
  message: string;
}
export interface Snapshot {
  account_email?: string | null;
  session: string;
  revision: number;
  provider: string;
  can_set_target: boolean;
  login: 'signing_out' | 'required' | 'signing_in' | 'failed' | 'ready' | null;
  read_error: string | null;
  connectivity: 'connecting' | 'online' | 'offline' | 'unavailable';
  last_sync: number | null;
  desired: number | null;
  confirmed: {
    device_name?: string | null;
    room_temperature: number;
    target_temperature: number;
    requested_temperature?: number | null;
    online: boolean;
    mode: { kind: string; raw?: string };
    revision: number;
    /** Device-origin application evidence correlated to a command; never cloud equality. */
    applied_command: number | null;
    /** Matching cloud observations only; cannot establish device application. */
    matched_command?: number | null;
    target_writable?: boolean;
  } | null;
  command: Command | null;
  in_flight: Command | null;
}
export type Scenario =
  | 'normal'
  | 'rejected'
  | 'transport_failure'
  | 'reply_lost'
  | 'ack_timeout'
  | 'device_timeout'
  | 'offline';
export interface SimConfig {
  scenario: Scenario;
  ack_ms: number;
  apply_ms: number;
}
export const temperature = (value: number | null | undefined) =>
  value == null ? '—' : (value / 10).toFixed(1);
