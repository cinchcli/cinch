export function ClipListSkeleton() {
  return (
    <div className="history-list" aria-hidden="true" data-testid="clip-list-skeleton">
      {[80, 66, 72, 58, 74, 60].map((width, i) => (
        <div key={i} className="clip-row">
          <div className="skeleton-shimmer" style={{ width: `${width}%`, height: 12, borderRadius: 4, background: 'var(--surface-2)' }} />
        </div>
      ))}
    </div>
  );
}
