import { expect, it } from 'vitest';
import { boundaryLabels } from './timelineLabels';
it('keeps uncrowded labels aligned with their boundaries', () => {
  expect(
    boundaryLabels([180, 270, 420, 720, 900], 690).map(
      (label) => label.position,
    ),
  ).toEqual([12.5, 18.75, (420 / 1440) * 100, 50, 62.5]);
});
it('spreads crowded edge labels on one line, without overlap or overflow', () => {
  for (const width of [240, 316, 690]) {
    const labels = boundaryLabels([10, 20, 30, 1410, 1420, 1430], width);
    const centers = labels.map((label) => (label.position / 100) * width);
    expect(centers[0]).toBeGreaterThanOrEqual(18);
    expect(centers[5]).toBeLessThanOrEqual(width - 18);
    for (let i = 1; i < centers.length; i++)
      expect(centers[i] - centers[i - 1]).toBeGreaterThanOrEqual(35.99);
    expect(labels.map((label) => label.minute)).toEqual([
      10, 20, 30, 1410, 1420, 1430,
    ]);
  }
});
it('returns labels to their boundary when enough space becomes available', () => {
  expect(boundaryLabels([420, 480], 300)[1].position).toBeGreaterThan(
    480 / 14.4,
  );
  expect(boundaryLabels([420, 480], 1200)[1].position).toBeCloseTo(480 / 14.4);
});
it('handles empty and single-boundary timelines', () => {
  expect(boundaryLabels([], 300)).toEqual([]);
  expect(boundaryLabels([720], 300)).toEqual([{ minute: 720, position: 50 }]);
});
