import type { Dashboard } from './dashboard.svelte';

export interface Progress {
  checkpoint: number;
  issue: boolean;
  retry: boolean;
  status: string;
  detail: string;
}

/** Milestones, not an estimate of elapsed time. Only device evidence fills the bar. */
export function commandProgress(
  dashboard: Pick<
    Dashboard,
    'state' | 'target' | 'localPending' | 'error' | 'streamConnected'
  >,
): Progress {
  const command = dashboard.state?.command;
  const phase = command?.phase;
  const result = (
    checkpoint: number,
    status: string,
    detail: string,
    issue = false,
    retry = false,
  ): Progress => ({ checkpoint, status, detail, issue, retry });

  // Selecting a new target completes the local-selection milestone immediately.
  // Debouncing, queueing and sending are one continuous Updating stage. Repeated
  // edits therefore leave the visual destination unchanged; only an edit after
  // acknowledgement/confirmation retreats, without retaining stale cloud evidence.
  if (dashboard.localPending)
    return result(
      1,
      'Updating',
      'Combining your adjustments before sending the latest target.',
    );
  if (dashboard.error)
    return result(1, 'Delivery unverified', dashboard.error, true, true);
  if (phase === 'queued')
    return result(
      1,
      'Updating',
      dashboard.state?.in_flight
        ? 'Your latest target is queued while the previous command finishes.'
        : 'Your latest target is ready to send.',
    );
  if (phase === 'sending')
    return result(
      1,
      'Updating',
      'The request is being sent. The server has not acknowledged it yet.',
    );
  if (phase === 'rejected')
    return result(1, 'Request rejected', command!.message, true, true);
  if (phase === 'transport_failed' || phase === 'timed_out')
    return result(1, 'Server reply unverified', command!.message, true, true);
  if (phase === 'unknown')
    return result(2, 'Device not yet verified', command!.message, true, true);

  if (dashboard.state?.connectivity === 'offline')
    return result(
      phase ? 2 : 0,
      'Device offline',
      'The last device reading is retained. Waiting for the device to reconnect.',
      true,
      !!command,
    );
  if (!dashboard.streamConnected && dashboard.state)
    return result(
      phase ? 2 : 0,
      'Live updates disconnected',
      'Reconnecting automatically. The latest device state is not yet verified.',
      true,
    );
  if (phase === 'accepted_by_cloud' || phase === 'awaiting_device')
    return result(
      2,
      'Server acknowledged',
      'The server accepted your target. Waiting for the device to confirm it.',
    );

  const confirmed = dashboard.state?.confirmed;
  if (
    confirmed &&
    dashboard.state?.connectivity === 'online' &&
    confirmed.target_temperature === dashboard.target &&
    (!command || phase === 'confirmed')
  ) {
    return result(
      3,
      'Synced with device',
      'The device has confirmed the displayed target temperature.',
    );
  }
  return result(
    0,
    'Waiting for device',
    'Waiting for a current device reading.',
  );
}
