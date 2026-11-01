export type Phase =
  | 'queued'
  | 'sending'
  | 'accepted_by_cloud'
  | 'awaiting_device'
  | 'confirmed'
  | 'rejected'
  | 'transport_failed'
  | 'timed_out'
  | 'unknown';
export interface Command {
  id: number;
  target: number;
  phase: Phase;
  message: string;
}
export interface Snapshot {
  session: string;
  revision: number;
  provider: string;
  can_set_target: boolean;
  read_error: string | null;
  connectivity: 'connecting' | 'online' | 'offline' | 'unavailable';
  last_sync: number | null;
  desired: number | null;
  confirmed: {
    room_temperature: number;
    target_temperature: number;
    online: boolean;
    mode: { kind: string; raw?: string };
    revision: number;
    applied_command: number | null;
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
