import { useState, useEffect, useCallback, useRef } from 'react';
import { commands, events } from '../bindings';
import { unwrap } from '../lib/tauri';
import { C, formatTime } from '../design';
import { sourcePillVars } from '../lib/sourceColor';
import { useMachineLabels } from '../lib/state/machineLabels';
import type { Device, SourceAlertSetting, SourceInfo } from '../bindings';
import { useLatestVersions } from '../lib/state/versions';
import { DeviceVersionCell } from './DeviceVersionCell';
import ConfirmDialog from '../ConfirmDialog';
import { CleanupDialog } from './CleanupDialog';
import { AddSshMachineDialog } from './AddSshMachineDialog';

// ─── Props ────────────────────────────────────────────────

interface DevicesPanelProps {
  currentDeviceID: string;
  currentMachineId?: string;
  onShowToast: (message: string) => void;
  onDeviceChange?: () => void;
}

// ─── Types ────────────────────────────────────────────────

type MergedEntry =
  | { kind: 'device'; device: Device }
  | { kind: 'source_only'; source: SourceInfo }
  | { kind: 'local' };

function settingsToAlertMap(settings: SourceAlertSetting[]): Record<string, boolean> {
  return Object.fromEntries(settings.map((s) => [s.source, s.alert_enabled]));
}

// ─── DevicesPanel ─────────────────────────────────────────

export function DevicesPanel({
  currentDeviceID,
  currentMachineId = '',
  onShowToast,
  onDeviceChange,
}: DevicesPanelProps) {
  const [devices, setDevices] = useState<Device[]>([]);
  const [sources, setSources] = useState<SourceInfo[]>([]);
  const [alertSettings, setAlertSettings] = useState<Record<string, boolean>>({});
  const { displayNames, setDisplayName } = useMachineLabels();
  const [loading, setLoading] = useState(true);
  const [editingSource, setEditingSource] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [savingNickname, setSavingNickname] = useState(false);
  const [openSettingsSource, setOpenSettingsSource] = useState<string | null>(null);
  const [confirmingRevokeId, setConfirmingRevokeId] = useState<string | null>(
    null,
  );
  const [cleanupHostname, setCleanupHostname] = useState<string | null>(null);
  const [nicknameError, setNicknameError] = useState<string | null>(null);
  const [showSshDialog, setShowSshDialog] = useState(false);
  const nicknameErrorTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const editInputRef = useRef<HTMLInputElement>(null);

  // Drives the per-device version badge in each row below; the hook keeps
  // itself in sync with the 6h background refresh emitted by Rust.
  const latestVersions = useLatestVersions();

  // ── Poll lifecycle ──────────────────────────────────────

  const fetchAll = useCallback(async () => {
    try {
      const [devs, srcs, alerts] = await Promise.allSettled([
        unwrap(commands.listDevices()),
        unwrap(commands.getSources()),
        unwrap(commands.getAllSourceAlertSettings()),
      ]);
      if (devs.status === 'fulfilled') setDevices(devs.value);
      if (srcs.status === 'fulfilled') setSources(srcs.value);
      if (alerts.status === 'fulfilled') {
        setAlertSettings(settingsToAlertMap(alerts.value));
      }
    } catch (e) {
      console.error('fetchAll failed:', e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchAll();
    const unsubPromise = events.devicesChanged.listen(() => {
      fetchAll();
    });
    return () => {
      unsubPromise.then((unsub) => unsub());
    };
  }, [fetchAll]);

  // ── Merge devices + sources ─────────────────────────────
  // Produce a combined list: paired devices first, then source-only
  // machines (sources without a matching device), then local.
  const merged: MergedEntry[] = (() => {
    const deviceSourceKeys = new Set(devices.map((d) => d.source_key));
    const currentDeviceIsPaired = devices.some(
      (d) =>
        d.id === currentDeviceID ||
        (currentMachineId !== '' && d.machine_id === currentMachineId),
    );
    const entries: MergedEntry[] = devices.map((d) => ({
      kind: 'device',
      device: d,
    }));

    for (const s of sources) {
      if (s.source === 'local') {
        // The "local" source represents clips made on this machine. When the
        // current machine is already in the paired-devices list, that row
        // already represents this machine — adding a synthetic "This device"
        // row would duplicate it. Only add the synthetic entry if the current
        // device isn't paired (e.g., transient relay/auth lag), and at most
        // once.
        if (
          !currentDeviceIsPaired &&
          !entries.some((e) => e.kind === 'local')
        ) {
          entries.push({ kind: 'local' });
        }
      } else if (!deviceSourceKeys.has(s.source)) {
        entries.push({ kind: 'source_only', source: s });
      }
    }

    return entries;
  })();

  // ── Nickname save ───────────────────────────────────────

  const saveDisplayName = useCallback(
    async (source: string, deviceId: string | undefined, nickname: string) => {
      setSavingNickname(true);
      setDisplayName(source, nickname);
      let success = true;
      try {
        if (deviceId) {
          await unwrap(commands.setDeviceNickname(deviceId, nickname));
          await fetchAll();
          onDeviceChange?.();
        }
      } catch (_e) {
        success = false;
        setNicknameError('Save failed — try again');
        if (nicknameErrorTimer.current)
          clearTimeout(nicknameErrorTimer.current);
        nicknameErrorTimer.current = setTimeout(
          () => setNicknameError(null),
          3000,
        );
      } finally {
        setSavingNickname(false);
        setEditingSource(null);
        if (success) {
          setOpenSettingsSource(null);
          setEditValue('');
        }
      }
    },
    [fetchAll, onDeviceChange, setDisplayName],
  );

  // ── Revoke ──────────────────────────────────────────────

  const revokeDevice = useCallback(
    async (deviceId: string) => {
      const hostname =
        devices.find((d) => d.id === deviceId)?.hostname || deviceId;
      try {
        await unwrap(commands.revokeDevice(deviceId));
        onShowToast('Device revoked');
        setCleanupHostname(hostname);
        await fetchAll();
        onDeviceChange?.();
      } catch (_e) {
        onShowToast('Failed to revoke device — try again');
      }
      setConfirmingRevokeId(null);
    },
    [devices, fetchAll, onDeviceChange, onShowToast],
  );

  const isAlertEnabled = useCallback(
    (source: string) => alertSettings[source] ?? true,
    [alertSettings],
  );

  const toggleAlert = useCallback(
    async (source: string, name: string) => {
      const next = !isAlertEnabled(source);
      setAlertSettings((prev) => ({ ...prev, [source]: next }));
      try {
        await unwrap(commands.setSourceAlertEnabled(source, next));
        onShowToast(next ? `Desktop alerts on for ${name}` : `Desktop alerts off for ${name}`);
      } catch (_e) {
        setAlertSettings((prev) => ({ ...prev, [source]: !next }));
        onShowToast('Failed to save alert setting');
      }
    },
    [isAlertEnabled, onShowToast],
  );

  // ── Nickname edit interaction ───────────────────────────

  const toggleSettings = useCallback((source: string, displayName: string) => {
    setOpenSettingsSource((current) => {
      const next = current === source ? null : source;
      if (next) {
        setEditingSource(source);
        setEditValue(displayName);
        setNicknameError(null);
      }
      return next;
    });
  }, []);

  const cancelEdit = useCallback(() => {
    setOpenSettingsSource(null);
    setEditingSource(null);
    setEditValue('');
    setNicknameError(null);
  }, []);

  const commitEdit = useCallback(
    (source: string, deviceId?: string) => {
      // Trimmed empty string is a valid save — it clears the nickname
      // (equivalent to `cinch device set-name --clear`), falling display
      // back to the system hostname.
      saveDisplayName(source, deviceId, editValue.trim());
    },
    [editValue, saveDisplayName],
  );

  // Focus input when editing starts
  useEffect(() => {
    if (editingSource && editInputRef.current) {
      editInputRef.current.focus();
      editInputRef.current.select();
    }
  }, [editingSource]);

  // Esc/Enter on revoke confirm dialog (keyboard accessibility)
  useEffect(() => {
    if (!confirmingRevokeId) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setConfirmingRevokeId(null);
      if (e.key === 'Enter') revokeDevice(confirmingRevokeId);
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [confirmingRevokeId, revokeDevice]);

  const lastSeen = (device: Device): string => {
    return device.last_push_at
      ? formatTime(Math.floor(new Date(device.last_push_at).getTime() / 1000))
      : 'never';
  };

  // ── Loading state ───────────────────────────────────────

  if (loading) {
    return (
      <div style={S.panel}>
        <header style={S.toolbar}>
          <div style={S.titleBlock}>
            <div className="skeleton-shimmer" style={S.skeletonBlockTitle} />
            <div className="skeleton-shimmer" style={S.skeletonBlockSubtitle} />
          </div>
        </header>
        <ul aria-label="Devices loading" style={{ ...S.list, listStyle: 'none', margin: 0, padding: 0 }}>
          {[0, 1, 2].map((i) => (
            <li key={i} style={{ ...S.rowWrap, listStyle: 'none' }}>
              <div style={S.skeletonRow}>
                <div style={S.skeletonRowMain}>
                <div
                  className="skeleton-shimmer"
                  style={{
                    ...S.skeletonBlock,
                    width: 10,
                    height: 10,
                    borderRadius: '50%',
                  }}
                />
                <div
                  className="skeleton-shimmer"
                  style={{
                    ...S.skeletonBlock,
                    flex: 1,
                    maxWidth: 280,
                    height: 14,
                    borderRadius: 4,
                  }}
                />
              </div>
              <div
                className="skeleton-shimmer"
                style={{
                  ...S.skeletonBlock,
                  width: 120,
                  height: 12,
                  borderRadius: 4,
                }}
              />
                <div style={S.skeletonActions}>
                  <div className="skeleton-shimmer" style={{ ...S.skeletonBlock, width: 72, height: 28, borderRadius: 6 }} />
                  <div className="skeleton-shimmer" style={{ ...S.skeletonBlock, width: 72, height: 28, borderRadius: 6 }} />
                </div>
              </div>
            </li>
          ))}
        </ul>
      </div>
    );
  }

  // ── Empty state ─────────────────────────────────────────

  if (merged.length === 0) {
    return (
      <div style={S.panel}>
        <header style={S.toolbar}>
          <div style={S.titleBlock}>
            <h1 style={S.pageTitle}>Devices</h1>
            <p style={S.pageSubtitle}>
              Paired devices and remote sources that send clips to this desk.
            </p>
          </div>
          <div style={S.toolbarAside}>
            <span style={S.statPillMuted}>0 connected</span>
          </div>
        </header>
        <div style={S.emptyState}>
          <div style={S.emptyCard}>
            <div style={S.emptyKicker}>Getting started</div>
            <div style={S.emptyHeading}>No devices yet</div>
            <div style={S.emptyBody}>
              Pair a device so clips can sync between your devices.
            </div>
            <code style={S.emptyCode}>cinch auth pair</code>
            <button
              type="button"
              className="btn-primary"
              style={S.emptyPrimaryBtn}
              onClick={() => setShowSshDialog(true)}
            >
              Add via SSH
            </button>
          </div>
          {showSshDialog && (
            <AddSshMachineDialog
              onClose={() => {
                setShowSshDialog(false);
                fetchAll();
              }}
              onShowToast={onShowToast}
            />
          )}
        </div>
      </div>
    );
  }

  const pairedCount = devices.length;
  const totalCount = merged.length;

  // The device being confirmed for revoke (needed for ConfirmDialog label)
  const confirmingDevice = confirmingRevokeId
    ? devices.find((d) => d.id === confirmingRevokeId)
    : undefined;
  const confirmingDisplayName = confirmingDevice
    ? confirmingDevice.nickname ||
      confirmingDevice.hostname ||
      confirmingRevokeId
    : confirmingRevokeId;

  // ── Device grid ──────────────────────────────────────────

  return (
    <div style={S.panel}>
      <header style={S.toolbar}>
        <div style={S.titleBlock}>
          <h1 style={S.pageTitle}>Devices</h1>
          <p style={S.pageSubtitle}>
            {pairedCount} paired · {totalCount} total · manage alerts and display names
          </p>
        </div>
        <div style={S.toolbarAside}>
          <span style={S.statPill}>
            <span style={S.statStrong}>{pairedCount}</span> paired
          </span>
          <span style={S.statPillMuted}>
            <span style={S.statStrong}>{totalCount}</span> in list
          </span>
          <button
            type="button"
            className="btn-primary"
            style={S.toolbarPrimary}
            onClick={() => setShowSshDialog(true)}
            aria-label="Add device via SSH"
          >
            Add via SSH
          </button>
        </div>
      </header>

      <ul aria-label="Devices" style={{ ...S.list, listStyle: 'none', margin: 0, padding: 0 }}>
        {merged.map((entry) => {
          if (entry.kind === 'local') {
            return (
              <li key="local" style={{ ...S.rowWrap, listStyle: 'none' }}>
                <div style={{ ...S.row, ...S.rowCurrent }}>
                  <div style={S.rowAccent} aria-hidden />
                  <div style={S.rowMain}>
                    <div style={S.rowTop}>
                      <span style={S.thisDeviceBadge}>This device</span>
                    </div>
                    <div style={S.cardName}>This device</div>
                    <div style={S.cardMeta}>Local clipboard · this Cinch instance</div>
                  </div>
                </div>
              </li>
            );
          }

          if (entry.kind === 'source_only') {
            const s = entry.source;
            const sourceLabel = s.source.replace(/^remote:/, '');
            const storedName = displayNames[s.source] ?? '';
            const displayName = storedName || sourceLabel;
            const settingsOpen = openSettingsSource === s.source;
            return (
              <li key={s.source} style={{ ...S.rowWrap, listStyle: 'none' }}>
                <div style={S.row}>
                  <div style={{ ...S.rowAccent, background: sourcePillVars(s.source).fg }} aria-hidden />
                  <div style={S.rowMain}>
                    <div style={S.rowTop}>
                      <span style={S.sourcePill}>{displayName}</span>
                      <span style={{ ...S.thisDeviceBadge, color: C.t4 }}>
                        Not paired
                      </span>
                    </div>
                    <div style={S.cardMeta}>
                      {s.clip_count} clips · last seen {formatTime(s.last_seen)}
                    </div>
                  </div>
                  <div style={S.rowActions}>
                    <button
                      type="button"
                      className="devices-btn"
                      style={S.customizeBtn}
                      onClick={() => toggleSettings(s.source, storedName)}
                      aria-expanded={settingsOpen}
                      aria-label={`Customize ${displayName}`}
                    >
                      Customize
                    </button>
                    <AlertToggle
                      enabled={isAlertEnabled(s.source)}
                      name={displayName}
                      onClick={() => toggleAlert(s.source, displayName)}
                    />
                  </div>
                </div>
                {settingsOpen && (
                  <DeviceSettingsPanel
                    name={displayName}
                    sourceLabel={sourceLabel}
                    editValue={editingSource === s.source ? editValue : storedName}
                    saving={savingNickname}
                    error={nicknameError}
                    inputRef={editInputRef}
                    onEditValueChange={setEditValue}
                    onCommit={() => commitEdit(s.source)}
                    onCancel={cancelEdit}
                  />
                )}
              </li>
            );
          }

          // entry.kind === "device"
          const device = entry.device;
          const isCurrentDevice =
            device.id === currentDeviceID ||
            (currentMachineId !== '' && device.machine_id === currentMachineId);
          const sourceKey = device.source_key ?? device.id ?? '';
          const storedName = displayNames[sourceKey] ?? device.nickname ?? '';
          const displayName = storedName || device.hostname || '';
          const alertSource = device.source_key;
          const alertName = device.hostname || displayName || 'device';
          const editLabelName = device.hostname || displayName || 'device';
          const settingsOpen = openSettingsSource === sourceKey;

          return (
            <li key={device.id} style={{ ...S.rowWrap, listStyle: 'none' }}>
              <div style={{ ...S.row, ...(isCurrentDevice ? S.rowCurrent : {}) }}>
                <div style={{ ...S.rowAccent, background: sourcePillVars(sourceKey).fg }} aria-hidden />
                <div style={S.rowMain}>
                  <div style={S.rowTop}>
                    <span style={S.sourcePill}>
                      {displayName || device.hostname || 'unknown'}
                    </span>
                    {isCurrentDevice && (
                      <span style={S.thisDeviceBadge}>This device</span>
                    )}
                  </div>
                  <div style={S.cardMeta}>
                    push relay · {device.clip_count ?? 0} clips · {lastSeen(device)}
                  </div>
                  <div style={{ ...S.cardMeta, marginTop: 4 }}>
                    <DeviceVersionCell
                      version={device.client_version ?? null}
                      clientType={device.client_type ?? null}
                      latest={latestVersions}
                      isOwnDesktop={
                        isCurrentDevice && device.client_type === 'desktop'
                      }
                    />
                  </div>
                </div>
                <div style={S.rowActions}>
                  <button
                    type="button"
                    className="devices-btn"
                    style={S.customizeBtn}
                    onClick={() => toggleSettings(sourceKey, storedName)}
                    aria-expanded={settingsOpen}
                    aria-label={`Customize ${editLabelName}`}
                  >
                    Customize
                  </button>
                  {alertSource && (
                    <AlertToggle
                      enabled={isAlertEnabled(alertSource)}
                      name={alertName}
                      onClick={() => toggleAlert(alertSource, alertName)}
                    />
                  )}
                  <button
                    type="button"
                    className="revoke-btn"
                    style={S.revokeBtn}
                    onClick={() => setConfirmingRevokeId(device.id ?? null)}
                  >
                    Revoke
                  </button>
                </div>
              </div>
              {settingsOpen && (
                <DeviceSettingsPanel
                  name={editLabelName}
                  sourceLabel={device.hostname ?? sourceKey}
                  editValue={editingSource === sourceKey ? editValue : storedName}
                  saving={savingNickname}
                  error={nicknameError}
                  inputRef={editInputRef}
                  onEditValueChange={setEditValue}
                  onCommit={() => commitEdit(sourceKey, device.id)}
                  onCancel={cancelEdit}
                />
              )}
            </li>
          );
        })}

        <li style={{ ...S.rowWrap, listStyle: 'none', borderBottom: 'none' }}>
          <button
            type="button"
            className="pair-row"
            style={S.pairRow}
            onClick={() => setShowSshDialog(true)}
            aria-label="Add device via SSH"
          >
            <span style={S.pairPlus} aria-hidden>
              +
            </span>
            <span style={S.pairRowText}>
              <span style={S.pairHeading}>Pair another device</span>
              <span style={S.pairBody}>SSH wizard · installs Cinch where you develop</span>
            </span>
          </button>
        </li>
      </ul>

      {showSshDialog && (
        <AddSshMachineDialog
          onClose={() => {
            setShowSshDialog(false);
            fetchAll();
          }}
          onShowToast={onShowToast}
        />
      )}

      {/* Revoke confirm dialog */}
      <ConfirmDialog
        open={confirmingRevokeId !== null}
        title="Revoke device?"
        body={
          <>
            Remove <strong>{confirmingDisplayName}</strong> from your account.
            It will no longer sync clips.
          </>
        }
        primaryLabel={`Revoke "${confirmingDisplayName}"`}
        secondaryLabel="Keep"
        tone="destructive"
        onConfirm={() => {
          if (confirmingRevokeId) revokeDevice(confirmingRevokeId);
        }}
        onCancel={() => setConfirmingRevokeId(null)}
      />

      {/* Post-revoke cleanup guide */}
      <CleanupDialog
        open={cleanupHostname !== null}
        hostname={cleanupHostname ?? ''}
        onClose={() => setCleanupHostname(null)}
      />
    </div>
  );
}

function AlertToggle({
  enabled,
  name,
  onClick,
}: {
  enabled: boolean;
  name: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="devices-btn"
      style={{ ...S.alertBtn, ...(enabled ? S.alertBtnOn : S.alertBtnOff) }}
      onClick={onClick}
      aria-label={`${enabled ? 'Turn desktop alerts off' : 'Turn desktop alerts on'} for ${name}`}
    >
      {enabled ? 'Alerts on' : 'Alerts off'}
    </button>
  );
}

function DeviceSettingsPanel({
  name,
  sourceLabel,
  editValue,
  saving,
  error,
  inputRef,
  onEditValueChange,
  onCommit,
  onCancel,
}: {
  name: string;
  sourceLabel: string;
  editValue: string;
  saving: boolean;
  error: string | null;
  inputRef: React.RefObject<HTMLInputElement | null>;
  onEditValueChange: (value: string) => void;
  onCommit: () => void;
  onCancel: () => void;
}) {
  return (
    <section
      style={S.settingsPanel}
      aria-label={`Device settings for ${name}`}
    >
      <div style={S.fieldBlock}>
        <div style={S.fieldHeader}>
          <span style={S.fieldLabel}>Name</span>
          <span style={S.fieldHint}>{sourceLabel}</span>
        </div>
        <input
          ref={inputRef}
          style={{
            ...S.nicknameInput,
            opacity: saving ? 0.5 : 1,
            pointerEvents: saving ? 'none' : 'auto',
          }}
          value={editValue}
          placeholder={sourceLabel}
          onChange={(e) => onEditValueChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              onCommit();
            }
            if (e.key === 'Escape') {
              e.preventDefault();
              onCancel();
            }
          }}
          maxLength={64}
          spellCheck={false}
          aria-label="Device display name"
        />
        {error && <span style={S.nicknameErrorText}>{error}</span>}
      </div>

      <div style={S.settingsActions}>
        <button
          type="button"
          className="devices-btn"
          style={S.settingsCancelBtn}
          onClick={onCancel}
          disabled={saving}
          aria-label={`Cancel customizing ${name}`}
        >
          Cancel
        </button>
        <button
          type="button"
          className="btn-primary"
          style={S.settingsSaveBtn}
          onClick={onCommit}
          disabled={saving}
          aria-label={`Save display name for ${name}`}
        >
          {saving ? 'Saving…' : 'Save'}
        </button>
      </div>
    </section>
  );
}

// ─── Styles ────────────────────────────────────────────────

const S: Record<string, React.CSSProperties> = {
  panel: {
    display: 'flex',
    flexDirection: 'column',
    flex: 1,
    minHeight: 0,
    background: C.bg,
  },

  toolbar: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 'var(--sp-lg)',
    padding: 'var(--sp-lg) var(--sp-xl)',
    borderBottom: `1px solid ${C.border}`,
    flexShrink: 0,
    flexWrap: 'wrap',
  },

  titleBlock: {
    display: 'flex',
    flexDirection: 'column',
    gap: 'var(--sp-xs)',
    minWidth: 0,
    flex: '1 1 200px',
  },

  pageTitle: {
    margin: 0,
    fontSize: 20,
    fontWeight: 650,
    letterSpacing: '-0.03em',
    color: C.t1,
    fontFamily: 'var(--font-body)',
    lineHeight: 1.2,
  },

  pageSubtitle: {
    margin: 0,
    fontSize: 12,
    fontWeight: 500,
    color: C.t3,
    fontFamily: 'var(--font-body)',
    lineHeight: 1.45,
    maxWidth: 440,
  },

  toolbarAside: {
    display: 'flex',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: 'var(--sp-sm)',
    flexShrink: 0,
  },

  statPill: {
    fontSize: 11,
    fontWeight: 600,
    color: C.t2,
    fontFamily: 'var(--font-mono)',
    fontVariantNumeric: 'tabular-nums',
    padding: 'var(--sp-xs) var(--sp-sm)',
    borderRadius: 9999,
    background: C.card2,
    border: `1px solid ${C.border}`,
    whiteSpace: 'nowrap',
  },

  statPillMuted: {
    fontSize: 11,
    fontWeight: 500,
    color: C.t3,
    fontFamily: 'var(--font-mono)',
    fontVariantNumeric: 'tabular-nums',
    padding: 'var(--sp-xs) var(--sp-sm)',
    borderRadius: 9999,
    background: 'transparent',
    border: `1px solid ${C.border}`,
    whiteSpace: 'nowrap',
  },

  statStrong: {
    color: C.t1,
    fontWeight: 700,
  },

  toolbarPrimary: {
    background: C.accent,
    color: C.accentOn,
    border: 'none',
    borderRadius: 'var(--radius-md)',
    padding: 'var(--sp-sm) var(--sp-md)',
    fontSize: 12,
    fontWeight: 600,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
    whiteSpace: 'nowrap',
  },

  list: {
    display: 'flex',
    flexDirection: 'column',
    flex: 1,
    minHeight: 0,
    overflowY: 'auto',
    padding: 'var(--sp-md) 0',
  },

  rowWrap: {
    borderBottom: `1px solid ${C.border}`,
  },

  row: {
    display: 'flex',
    alignItems: 'flex-start',
    gap: 'var(--sp-md)',
    padding: 'var(--sp-md) var(--sp-xl)',
    minHeight: 72,
    boxSizing: 'border-box',
    background: C.bg,
    transition: 'background 120ms ease',
  },

  rowCurrent: {
    background: C.selected,
  },

  rowAccent: {
    width: 3,
    alignSelf: 'stretch',
    borderRadius: 2,
    flexShrink: 0,
    marginTop: 2,
    marginBottom: 2,
    background: C.borderHover,
  },

  rowMain: {
    flex: 1,
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
    gap: 'var(--sp-xs)',
  },

  rowTop: {
    display: 'flex',
    alignItems: 'center',
    gap: 'var(--sp-sm)',
    flexWrap: 'wrap',
  },

  rowActions: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'flex-end',
    gap: 'var(--sp-sm)',
    flexWrap: 'wrap',
    flexShrink: 0,
    paddingTop: 2,
  },

  pairRow: {
    width: '100%',
    display: 'flex',
    alignItems: 'center',
    gap: 'var(--sp-md)',
    padding: 'var(--sp-lg) var(--sp-xl)',
    margin: '0 var(--sp-md)',
    boxSizing: 'border-box',
    background: 'transparent',
    border: `1px dashed ${C.border}`,
    borderRadius: 'var(--radius-lg)',
    cursor: 'pointer',
    textAlign: 'left',
    font: 'inherit',
    color: 'inherit',
  },

  pairPlus: {
    width: 36,
    height: 36,
    borderRadius: 'var(--radius-md)',
    background: C.card2,
    border: `1px solid ${C.border}`,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    fontSize: 20,
    fontWeight: 500,
    color: C.t2,
    flexShrink: 0,
    lineHeight: 1,
  },

  pairRowText: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    minWidth: 0,
  },

  sourcePill: {
    fontSize: 13,
    fontWeight: 500,
    color: C.t1,
    letterSpacing: '-0.01em',
    fontFamily: 'var(--font-body)',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    maxWidth: 220,
  },

  thisDeviceBadge: {
    background: C.card2,
    fontSize: 11,
    fontWeight: 600,
    color: C.t3,
    padding: '2px var(--sp-sm)',
    borderRadius: 'var(--radius-sm)',
    fontFamily: 'var(--font-body)',
    whiteSpace: 'nowrap',
  },

  cardName: {
    fontSize: 15,
    fontWeight: 600,
    color: C.t1,
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    letterSpacing: '-0.012em',
    lineHeight: 1.4,
  },

  nicknameInput: {
    fontSize: 13,
    fontWeight: 500,
    color: C.t1,
    letterSpacing: '-0.012em',
    background: C.card,
    border: `1px solid ${C.borderHover}`,
    borderRadius: 'var(--radius-md)',
    padding: 'var(--sp-sm)',
    outline: 'none',
    lineHeight: 1.4,
    width: '100%',
    boxSizing: 'border-box',
  },

  nicknameErrorText: {
    fontSize: 12,
    color: C.error,
    fontFamily: 'var(--font-body)',
  },

  cardMeta: {
    fontSize: 12,
    fontWeight: 500,
    color: C.t3,
    fontFamily: 'var(--font-mono)',
    fontVariantNumeric: 'tabular-nums',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },

  customizeBtn: {
    background: 'transparent',
    color: C.t2,
    border: `1px solid ${C.border}`,
    borderRadius: 'var(--radius-md)',
    padding: 'var(--sp-xs) var(--sp-sm)',
    fontSize: 12,
    fontWeight: 500,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
    whiteSpace: 'nowrap',
  },

  settingsActions: {
    display: 'flex',
    justifyContent: 'flex-end',
    gap: 'var(--sp-sm)',
    marginTop: 'var(--sp-xs)',
  },

  settingsCancelBtn: {
    background: 'transparent',
    color: C.t2,
    border: `1px solid ${C.border}`,
    borderRadius: 'var(--radius-md)',
    padding: 'var(--sp-xs) var(--sp-md)',
    fontSize: 12,
    fontWeight: 500,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
    whiteSpace: 'nowrap',
  },

  settingsSaveBtn: {
    background: C.accent,
    color: C.accentOn,
    border: `1px solid ${C.accent}`,
    borderRadius: 'var(--radius-md)',
    padding: 'var(--sp-xs) var(--sp-md)',
    fontSize: 12,
    fontWeight: 600,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
    whiteSpace: 'nowrap',
  },

  settingsPanel: {
    marginLeft: 'var(--sp-xl)',
    marginRight: 'var(--sp-lg)',
    marginBottom: 'var(--sp-md)',
    padding: 'var(--sp-md)',
    borderRadius: 'var(--radius-md)',
    border: `1px solid ${C.border}`,
    background: C.card2,
    display: 'flex',
    flexDirection: 'column',
    gap: 'var(--sp-md)',
  },

  fieldBlock: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },

  fieldHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 8,
  },

  fieldLabel: {
    fontSize: 11,
    fontWeight: 600,
    letterSpacing: '0.01em',
    color: C.t2,
    fontFamily: 'var(--font-body)',
  },

  fieldHint: {
    minWidth: 0,
    color: C.t3,
    fontSize: 11,
    fontWeight: 600,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    fontFamily: 'var(--font-mono)',
  },

  revokeBtn: {
    background: 'transparent',
    color: C.error,
    border: '1px solid color-mix(in srgb, var(--error) 25%, transparent)',
    borderRadius: 'var(--radius-md)',
    padding: 'var(--sp-xs) var(--sp-sm)',
    fontSize: 12,
    fontWeight: 500,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
    whiteSpace: 'nowrap',
  },
  alertBtn: {
    background: 'transparent',
    borderRadius: 'var(--radius-md)',
    padding: 'var(--sp-xs) var(--sp-sm)',
    fontSize: 12,
    fontWeight: 500,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
    whiteSpace: 'nowrap',
  },
  alertBtnOn: {
    color: C.t2,
    border: `1px solid ${C.border}`,
  },
  alertBtnOff: {
    color: C.t4,
    border: `1px solid ${C.border}`,
  },

  pairHeading: {
    fontSize: 13,
    fontWeight: 600,
    color: C.t1,
    fontFamily: 'var(--font-body)',
  },

  pairBody: {
    fontSize: 12,
    fontWeight: 500,
    color: C.t3,
    fontFamily: 'var(--font-body)',
  },

  emptyState: {
    flex: 1,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 'var(--sp-xl)',
    minHeight: 0,
  },
  emptyCard: {
    maxWidth: 380,
    width: '100%',
    padding: 'var(--sp-xl)',
    borderRadius: 'var(--radius-lg)',
    border: `1px solid ${C.border}`,
    background: C.card,
    textAlign: 'center',
    display: 'flex',
    flexDirection: 'column',
    gap: 'var(--sp-sm)',
    alignItems: 'center',
  },
  emptyKicker: {
    fontSize: 11,
    fontWeight: 500,
    letterSpacing: '0.01em',
    color: C.t4,
    fontFamily: 'var(--font-body)',
  },
  emptyHeading: {
    fontSize: 17,
    fontWeight: 600,
    color: C.t1,
    fontFamily: 'var(--font-body)',
    letterSpacing: '-0.02em',
  },
  emptyBody: {
    fontSize: 13,
    fontWeight: 500,
    color: C.t3,
    fontFamily: 'var(--font-body)',
    lineHeight: 1.55,
  },
  emptyCode: {
    fontSize: 12,
    fontFamily: 'var(--font-mono)',
    color: C.t2,
    padding: 'var(--sp-sm) var(--sp-md)',
    borderRadius: 'var(--radius-sm)',
    background: C.card2,
    border: `1px solid ${C.border}`,
  },
  emptyPrimaryBtn: {
    marginTop: 'var(--sp-sm)',
    background: C.accent,
    color: C.accentOn,
    border: 'none',
    borderRadius: 'var(--radius-md)',
    padding: 'var(--sp-sm) var(--sp-lg)',
    fontSize: 12,
    fontWeight: 600,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
  },

  skeletonRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 'var(--sp-md)',
    padding: 'var(--sp-md) var(--sp-xl)',
    minHeight: 72,
    borderBottom: `1px solid ${C.border}`,
  },
  skeletonRowMain: {
    flex: 1,
    display: 'flex',
    alignItems: 'center',
    gap: 'var(--sp-md)',
    minWidth: 0,
  },
  skeletonActions: {
    display: 'flex',
    gap: 'var(--sp-sm)',
  },
  skeletonBlock: {
    background: C.card2,
    opacity: 0.5,
  },
  skeletonBlockTitle: {
    height: 22,
    width: 140,
    borderRadius: 4,
    background: C.card2,
    opacity: 0.55,
  },
  skeletonBlockSubtitle: {
    height: 12,
    width: 280,
    borderRadius: 4,
    background: C.card2,
    opacity: 0.35,
  },
};
