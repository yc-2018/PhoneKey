export function signalLevel(rssi: number | null): number {
  if (rssi == null) return 0;
  if (rssi >= -60) return 4;
  if (rssi >= -72) return 3;
  if (rssi >= -84) return 2;
  if (rssi >= -95) return 1;
  return 0;
}

export function RssiBars({
  rssi,
  large = false,
}: {
  rssi: number | null;
  large?: boolean;
}) {
  const level = signalLevel(rssi);
  return (
    <span className={`rssi-bars ${large ? "is-large" : ""} level-${level}`}>
      {[0, 1, 2, 3].map((index) => (
        <span
          key={index}
          className={`rssi-bar ${index < level ? "active" : ""}`}
          style={{ height: `${5 + index * 3}px` }}
        />
      ))}
    </span>
  );
}

export function formatRssi(rssi: number | null): string {
  return rssi == null ? "-- dBm" : `${Math.round(rssi)} dBm`;
}
