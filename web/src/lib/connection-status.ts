import type { Dashboard } from './dashboard.svelte';
import { temperature, type Snapshot } from './types';
import type { StatusIcon } from './status-icon-sequence.svelte';

export function connectionStatus(
  dashboard: Pick<
    Dashboard,
    | 'state'
    | 'target'
    | 'localPending'
    | 'debouncing'
    | 'error'
    | 'streamConnected'
    | 'cloudAcknowledged'
    | 'readError'
  >,
) {
  const command = dashboard.state?.command;
  const phase = command?.phase;
  const result = (
    icon: StatusIcon,
    title: string,
    detail: string,
    retry = false,
  ) => ({ icon, title, detail, retry });
  let server;
  if (dashboard.error)
    server = result('error', 'Delivery unverified', dashboard.error, true);
  else if (dashboard.localPending)
    server = !dashboard.streamConnected
      ? result(
          'error',
          'Server status unavailable',
          'The live connection was interrupted. The latest request has not been verified.',
        )
      : result(
          dashboard.debouncing ? 'adjusting' : 'sending',
          'Updating server',
          dashboard.debouncing
            ? 'Combining your adjustments before sending the latest target.'
            : 'Your latest target is waiting to be accepted by the server.',
        );
  else if (phase === 'queued' || phase === 'sending')
    server = !dashboard.streamConnected
      ? result(
          'error',
          'Server status unavailable',
          'The live connection was interrupted. Acceptance of this request is not yet verified.',
        )
      : result(
          'sending',
          'Updating server',
          'Sending the latest target. Server acceptance is not yet verified.',
        );
  else if (phase === 'rejected')
    server = result('error', 'Server rejected request', command!.message, true);
  else if (
    command &&
    command.target === dashboard.target &&
    (dashboard.cloudAcknowledged ||
      phase === 'accepted_by_cloud' ||
      phase === 'awaiting_device')
  )
    server = result(
      'synced',
      'Accepted by server',
      'The server explicitly accepted this target. This does not confirm that the physical thermostat has applied it.',
    );
  else if (command)
    server = result(
      'error',
      'Server acceptance unverified',
      'No successful server reply was verified for this request. Matching cloud temperatures do not prove acceptance or physical application.',
      true,
    );
  else
    server = result(
      'waiting',
      'No pending request',
      'No temperature request is awaiting server acceptance. A cloud reading alone does not produce an acceptance checkmark.',
    );

  const cloud = dashboard.state?.confirmed;
  const reading = cloud
    ? `Last cloud status: ${temperature(cloud.target_temperature)}°C.`
    : 'No cloud reading is available yet.';
  let detail =
    'Physical thermostat synchronization cannot currently be verified. ' +
    reading;
  if (dashboard.readError) detail += ' ' + dashboard.readError;
  else if (!dashboard.streamConnected && dashboard.state)
    detail += ' Live updates are disconnected.';
  else if (dashboard.state?.connectivity === 'offline')
    detail += ' The cloud reports the thermostat offline.';
  else detail += ' Cloud values can update before the physical thermostat.';
  const verified =
    !dashboard.localPending &&
    !dashboard.error &&
    dashboard.streamConnected &&
    deviceApplicationVerified(dashboard.state, dashboard.target);
  return {
    server,
    thermostat: {
      state: verified ? 'verified' : 'unverified',
      title: verified ? 'Thermostat confirmed' : 'Thermostat unverified',
      detail: verified
        ? 'The device reported applying this target, with evidence linked to the latest command.'
        : detail,
    },
    reading,
  };
}

/**
 * Device confirmation boundary. Providers may supply applied_command only from
 * verified device-origin evidence. Cloud equality/matched_command is insufficient.
 * OCSTAT currently never supplies this evidence; the simulator exercises it.
 */
export function deviceApplicationVerified(
  state: Snapshot | null,
  target: number | null,
): boolean {
  const command = state?.command;
  const reading = state?.confirmed;
  return !!(
    command &&
    reading &&
    !state?.read_error &&
    state?.connectivity === 'online' &&
    reading.online &&
    command.phase === 'confirmed' &&
    reading.applied_command === command.id &&
    command.target === target &&
    reading.target_temperature === target
  );
}
