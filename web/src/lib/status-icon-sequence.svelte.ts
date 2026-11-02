export type StatusIcon =
  'waiting' | 'adjusting' | 'sending' | 'cloud' | 'synced' | 'error';
export const ICON_TRANSITION_MS = 220;
// Allows frame scheduling slack when CSS completion events are unavailable.
export const ICON_FALLBACK_MS = ICON_TRANSITION_MS + 80;
const stages: StatusIcon[] = ['adjusting', 'sending', 'cloud', 'synced'];

/** A short presentation queue. It never delays or changes actual thermostat state. */
export class StatusIconSequence {
  display = $state<StatusIcon>('waiting');
  private intent: string | undefined;
  private requested: StatusIcon = 'waiting';
  private queue: StatusIcon[] = [];
  private timer: ReturnType<typeof setTimeout> | undefined;
  private disposed = false;

  update(
    next: StatusIcon,
    intent: string,
    cloudAcknowledged: boolean,
    reducedMotion = false,
  ) {
    if (this.disposed) return;
    const first = this.intent === undefined;
    const changedIntent = this.intent !== intent;
    this.intent = intent;
    if (first || reducedMotion) {
      this.clear();
      this.display = next;
      this.requested = next;
      return;
    }
    const from = stages.indexOf(this.requested);
    const to = stages.indexOf(next);
    // New edits, errors, loss of evidence and recovery supersede queued completion.
    // Retargeting persistent CSS layers blends from their current opacity.
    if (
      changedIntent ||
      next === 'error' ||
      next === 'waiting' ||
      this.requested === 'error' ||
      to < from
    ) {
      this.queue = [];
      this.requested = next;
      if (this.display !== next) {
        clearTimeout(this.timer);
        this.timer = undefined;
        this.display = next;
        this.hold();
      }
      return;
    }
    if (next === this.requested) return;
    this.requested = next;
    // Fill quick forward transitions only within an active adjustment. A cloud
    // icon requires an observed acknowledgement, including after a lost reply.
    const path =
      from >= 0 && to > from
        ? stages
            .slice(from + 1, to + 1)
            .filter(
              (icon) =>
                icon !== 'cloud' || cloudAcknowledged || next === 'cloud',
            )
        : [next];
    this.queue.push(...path);
    this.advance();
  }
  private advance() {
    if (this.timer !== undefined || this.disposed) return;
    const next = this.queue.shift();
    if (!next) return;
    this.display = next;
    this.hold();
  }
  finishTransition(icon: StatusIcon) {
    if (this.disposed || this.timer === undefined || icon !== this.display)
      return;
    clearTimeout(this.timer);
    this.timer = undefined;
    this.advance();
  }
  private hold() {
    const icon = this.display;
    this.timer = setTimeout(
      () => this.finishTransition(icon),
      ICON_FALLBACK_MS,
    );
  }
  private clear() {
    clearTimeout(this.timer);
    this.timer = undefined;
    this.queue = [];
  }
  dispose() {
    this.disposed = true;
    this.clear();
  }
}
