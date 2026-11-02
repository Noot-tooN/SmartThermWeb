/** Keep changeover times on one line. Leaders connect displaced labels to their exact boundary. */
export function boundaryLabels(minutes: number[], width: number) {
  const gap = Math.min(36, width / Math.max(1, minutes.length));
  const edge = gap / 2;
  const centers = minutes.map((minute) =>
    Math.max(edge, Math.min(width - edge, (minute / 1440) * width)),
  );
  for (let i = 1; i < centers.length; i++)
    centers[i] = Math.max(centers[i], centers[i - 1] + gap);
  if (centers.length)
    centers[centers.length - 1] = Math.min(
      centers[centers.length - 1],
      width - edge,
    );
  for (let i = centers.length - 2; i >= 0; i--)
    centers[i] = Math.min(centers[i], centers[i + 1] - gap);
  return minutes.map((minute, i) => ({
    minute,
    position: (centers[i] / width) * 100,
  }));
}
