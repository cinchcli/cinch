import { useEffect, useState, useCallback, useRef, useMemo } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { LogicalSize } from '@tauri-apps/api/dpi';
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import { commands, events } from './bindings';
import type { LocalClip, SourceInfo, Device } from './bindings';
import { unwrap } from './lib/tauri';
import { groupByTimeBucket } from './lib/timeBuckets';
import { type ClipFilter } from './lib/clipFilters';
import { physicalKey, isImeComposition } from './lib/keyboard';
import { matchesAccelerator, formatShortcutDisplay, DEFAULT_ACTION_SHORTCUTS, type ActionShortcuts } from './lib/keymap';
import { loadMachineDisplayNames } from './lib/machineDisplayNames';
import { useMachineLabels } from './lib/state/machineLabels';
import { useTheme } from './lib/state/theme';
import { C } from './design';
import { useAuthState, retryAuth, signOut, type AuthProgress, type AuthErrorReason } from './lib/state/auth';
import { useNotifyOnRemoteLogin } from './lib/settings';
import SettingsPane, { type SettingsTab } from './SettingsPane';
import { AdoptedAuthToast } from './components/AdoptedAuthToast';
import { OfflineQueueDroppedToast } from './components/OfflineQueueDroppedToast';
import { ClipDecryptFailedToast } from './components/ClipDecryptFailedToast';
import { SendToast } from './components/SendToast';
import { AddRelayDialog } from './components/AddRelayDialog';
import { Rail, type RailPanel } from './components/Rail';
import { SearchBar, type DeviceOption } from './components/SearchBar';
import { buildDeviceOptions } from './lib/deviceOptions';
import { ClipList } from './components/ClipList';
import { ClipListSkeleton } from './components/ClipListSkeleton';
import { ClipDetail } from './components/ClipDetail';
import { EditClipModal } from './components/EditClipModal';
import { PinnedPanel } from './components/PinnedPanel';
import { DevicesPanel } from './components/DevicesPanel';
import { GettingStartedCard } from './components/GettingStartedCard';
import { OnboardingScreen } from './components/OnboardingScreen';
import { dialogStyles } from './components/dialogPrimitives';
import { IconCopy, IconTrash, IconX } from './icons';
import { UpdateBanner } from './components/UpdateBanner';
import { useLatestVersions } from './lib/state/versions';
import packageJson from '../package.json';
import './App.css';

function handleWindowDrag(e: React.MouseEvent) {
  const target = e.target as HTMLElement;
  if (!target.closest('button, input, a, textarea')) {
    void commands.snapDragStart();
    void getCurrentWindow().startDragging();
  }
}

const WINDOW_PRESETS = {
  compact:  { width: 760,  height: 480 },
  standard: { width: 960,  height: 600 },
  spacious: { width: 1120, height: 720 },
} as const;

function App() {
  const { mode: themeMode, setMode: setThemeMode } = useTheme();

  useEffect(() => {
    const saved = localStorage.getItem('cinch-window-size') as keyof typeof WINDOW_PRESETS | null;
    const preset = saved && saved in WINDOW_PRESETS ? saved : 'standard';
    const { width, height } = WINDOW_PRESETS[preset];
    void getCurrentWindow().setSize(new LogicalSize(width, height));
  }, []);

  const auth = useAuthState();
  const [notifyOnRemoteLogin] = useNotifyOnRemoteLogin();
  const latestVersions = useLatestVersions();
  const currentDesktopVersion = packageJson.version;

  // CLI handoff (cinch://login from `cinch auth login`). Shown above all
  // auth-state branches so the dialog opens regardless of LocalOnly /
  // Authenticating / Authenticated.
  const [handoffRelay, setHandoffRelay] = useState<string | null>(null);
  useEffect(() => {
    const unsubP = events.cliHandoffRequested.listen((e) => {
      setHandoffRelay(e.payload.relay_url || '');
    });
    return () => { unsubP.then((f) => f()); };
  }, []);

  // OS notification when a remote login request arrives (device_code_pending).
  // Re-runs when notifyOnRemoteLogin changes so toggling the setting takes
  // effect immediately: when disabled the effect returns early (no listener
  // is registered), and the cleanup from the previous run unsubscribes.
  useEffect(() => {
    if (!notifyOnRemoteLogin) return;
    let cancelled = false;
    let unsub: (() => void) | null = null;
    (async () => {
      let granted = await isPermissionGranted();
      if (!granted) {
        const result = await requestPermission();
        granted = result === 'granted';
      }
      if (cancelled || !granted) return;
      unsub = await events.deviceCodePending.listen((e) => {
        const p = e.payload;
        sendNotification({
          title: 'Approve remote login?',
          body: `From ${p.hostname}${p.source_region ? ` (${p.source_region})` : ''}\nCode: ${p.user_code}`,
        });
      });
    })();
    return () => { cancelled = true; unsub?.(); };
  }, [notifyOnRemoteLogin]);

  const [_status, setStatus] = useState('connecting');
  const [clips, setClips] = useState<LocalClip[]>([]);
  // False until the first clip fetch settles, so the inbox shows a loading
  // skeleton instead of flashing the empty state before clips arrive.
  const [clipsLoaded, setClipsLoaded] = useState(false);
  const [sources, setSources] = useState<SourceInfo[]>([]);
  const [selectedClip, setSelectedClip] = useState<LocalClip | null>(null);
  const [selectedSource, setSelectedSource] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [devices, setDevices] = useState<Device[]>([]);
  const { tagColors, displayNames } = useMachineLabels();
  const [newSourcePrompt, setNewSourcePrompt] = useState<string | null>(null);
  const [pinNoteDialog, setPinNoteDialog] = useState<{ clip: LocalClip } | null>(null);
  const [editDialog, setEditDialog] = useState<{ clip: LocalClip } | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>('general');
  const openSettings = (tab: SettingsTab = 'general') => {
    setSettingsTab(tab);
    setShowSettings(true);
  };
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [activePanel, setActivePanel] = useState<RailPanel>('inbox');
  const [activeFilter, setActiveFilter] = useState<ClipFilter>('all');
  const searchRef = useRef<HTMLInputElement>(null);
  const clipListRef = useRef<HTMLDivElement>(null);
  const copyRecencyRef = useRef<Map<string, number>>(new Map());

  const clipRecency = useCallback((clip: LocalClip) => {
    const override = copyRecencyRef.current.get(clip.id);
    if (override !== undefined) return override;
    return clip.received_at && clip.received_at > 0 ? clip.received_at : clip.created_at;
  }, []);

  const applyInboxRecency = useCallback((list: LocalClip[]) => {
    if (copyRecencyRef.current.size === 0) return list;
    const updated = list.map((clip) => {
      const override = copyRecencyRef.current.get(clip.id);
      return override !== undefined ? { ...clip, received_at: override } : clip;
    });
    updated.sort((a, b) => {
      const ra = clipRecency(a);
      const rb = clipRecency(b);
      if (rb !== ra) return rb - ra;
      return b.created_at - a.created_at;
    });
    return updated;
  }, [clipRecency]);

  const [toast, setToast] = useState<{ message: string; icon: 'copy' | 'trash' | 'error' } | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const showToast = useCallback((message: string, icon: 'copy' | 'trash' | 'error') => {
    if (toastTimer.current) clearTimeout(toastTimer.current);
    setToast({ message, icon });
    toastTimer.current = setTimeout(() => setToast(null), 1800);
  }, []);

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(searchQuery), 200);
    return () => clearTimeout(timer);
  }, [searchQuery]);

  const refreshClips = useCallback(async () => {
    try {
      if (activePanel === 'pinned') {
        const pinned = await unwrap(commands.listPinnedClips());
        setClips(pinned);
        return;
      }
      let finalQuery = debouncedQuery;
      if (selectedSource) {
        finalQuery = `from:${selectedSource} ${finalQuery}`.trim();
      }
      if (activeFilter !== 'all') {
        finalQuery = `type:${activeFilter} ${finalQuery}`.trim();
      }
      const results = await unwrap(commands.listClips(finalQuery, 500));
      setClips(applyInboxRecency(results));
    } catch (e) {
      console.error('failed to load clips:', e);
    } finally {
      setClipsLoaded(true);
    }
  }, [activePanel, selectedSource, debouncedQuery, activeFilter, applyInboxRecency]);

  const refreshSources = useCallback(async () => {
    try {
      setSources(await unwrap(commands.getSources()));
    } catch (e) {
      console.error(e);
    }
  }, []);

  const refreshDevices = useCallback(async () => {
    try {
      setDevices(await unwrap(commands.listDevices()));
    } catch (e) {
      console.error(e);
    }
  }, []);

  const handleNewSourceResponse = async (source: string, enable: boolean) => {
    await unwrap(commands.setSourceAutoCopy(source, enable));
    setNewSourcePrompt(null);
  };

  useEffect(() => {
    if (auth.variant !== 'Authenticated') return;
    const timer = setTimeout(() => {
      refreshClips();
      refreshSources();
      refreshDevices();
    }, 1000);
    return () => clearTimeout(timer);
  }, [auth.variant, refreshClips, refreshSources, refreshDevices]);

  useEffect(() => { refreshClips(); }, [refreshClips]);

  // Scroll selected clip into view when navigating with keyboard
  useEffect(() => {
    if (!selectedClip || !clipListRef.current) return;
    const el = clipListRef.current.querySelector<HTMLElement>(`[data-id="${selectedClip.id}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  }, [selectedClip]);

  useEffect(() => {
    commands.getWsStatus().then(setStatus).catch(() => {});
    const unsubs = [
      events.wsStatus.listen((e) => setStatus(e.payload)),
      events.clipReceived.listen(() => { refreshClips(); refreshSources(); }),
      events.remoteClipReceived.listen(() => { refreshClips(); refreshSources(); }),
      events.clipDeleted.listen(() => { refreshClips(); refreshSources(); }),
      events.devicesChanged.listen(() => { refreshDevices(); }),
      events.clipPinned.listen(() => { refreshClips(); }),
      events.newSourceDetected.listen((e) => {
        setNewSourcePrompt(e.payload);
      }),
      events.trayOpenSettings.listen(() => openSettings()),
    ];
    return () => { unsubs.forEach((p) => p.then((f) => f())); };
  }, [refreshClips, refreshSources, refreshDevices]);

  // OS notification when a clip arrives from another device. The per-source
  // toggle in DevicesPanel (Alerts on/off) gates this — defaults to on.
  // Permission is requested lazily on first incoming clip rather than at
  // launch so users who never receive remote clips aren't prompted.
  useEffect(() => {
    let cancelled = false;
    let unsub: (() => void) | null = null;
    let permissionChecked = false;
    let permissionGranted = false;

    (async () => {
      unsub = await events.remoteClipReceived.listen(async (e) => {
        const clip = e.payload;
        try {
          const enabled = await unwrap(commands.getSourceAlertEnabled(clip.source));
          if (!enabled) return;
        } catch {
          return;
        }
        if (cancelled) return;
        if (!permissionChecked) {
          permissionChecked = true;
          let granted = await isPermissionGranted();
          if (!granted) {
            const result = await requestPermission();
            granted = result === 'granted';
          }
          permissionGranted = granted;
        }
        if (!permissionGranted || cancelled) return;
        const names = loadMachineDisplayNames();
        const sourceLabel = clip.source.replace(/^remote:/, '');
        const title = names[clip.source] ?? sourceLabel;
        const body =
          clip.content_type === 'image'
            ? `New image · ${clip.byte_size.toLocaleString()} B`
            : `New clipboard · ${clip.byte_size.toLocaleString()} B`;
        sendNotification({ title, body });
      });
    })();

    return () => { cancelled = true; unsub?.(); };
  }, []);

  useEffect(() => {
    const unsubBlur = getCurrentWindow().listen('tauri://blur', () => {
      setSelectedClip(null);
      setSearchQuery('');
      setDebouncedQuery('');
    });
    return () => { unsubBlur.then((f) => f()); };
  }, []);

  const handleSaveImage = useCallback(async (clip: LocalClip) => {
    try {
      const path = await unwrap(commands.saveImageToFile(clip.id));
      if (path) console.info('[save-image] wrote', path);
    } catch (e) {
      console.error('[save-image] failed', e);
    }
  }, []);

  const bumpClipRecency = useCallback((clip: LocalClip) => {
    const now = Math.floor(Date.now() / 1000);
    copyRecencyRef.current.set(clip.id, now);
    if (activePanel !== 'pinned') {
      setClips((prev) => applyInboxRecency(prev));
    }
  }, [activePanel, applyInboxRecency]);

  const finishCopy = useCallback((clip: LocalClip, message: string) => {
    void unwrap(commands.showCopyToast(message))
      .catch((e) => {
        console.error('show copy toast failed:', e);
        showToast(message, 'copy');
      });
    bumpClipRecency(clip);
    setSearchQuery('');
    setDebouncedQuery('');
    setSelectedClip(null);
    void refreshClips();
    void commands.focusPreviousApp();
  }, [refreshClips, showToast]);

  const copyClip = useCallback((clip: LocalClip) => {
    if (clip.content_type === 'image') {
      void unwrap(commands.copyImageToClipboard(clip.id))
        .catch((e) => console.error('copy image failed:', e));
      finishCopy(clip, 'Copied image to clipboard');
    } else {
      void unwrap(commands.copyClipToClipboard(clip.content))
        .catch((e) => console.error('copy clip failed:', e));
      finishCopy(clip, 'Copied text to clipboard');
    }
  }, [finishCopy]);

  // Broadcasts the clip to all of the user's devices.
  const sendClip = useCallback(async (clip: LocalClip) => {
    try {
      await unwrap(commands.sendClip(clip.id));
      refreshClips();
      showToast('Sent', 'copy');
    } catch (e) {
      console.error('sendClip failed', e);
      showToast(e instanceof Error ? e.message : 'Send failed', 'error');
    }
  }, [refreshClips, showToast]);

  const handleEdit = useCallback(async (clip: LocalClip, newContent: string) => {
    try {
      const newClip = await unwrap(commands.editClip(clip.id, newContent));
      setEditDialog(null);
      await refreshClips();
      setSelectedClip(newClip);
      showToast('Edited & copied', 'copy');
    } catch (e) {
      // editClip can fail after persisting the new clip (e.g. clipboard write).
      // Close the modal and surface the error rather than leaving it stuck open.
      console.error('editClip failed', e);
      setEditDialog(null);
      showToast(e instanceof Error ? e.message : 'Edit failed', 'error');
    }
  }, [refreshClips, showToast]);

  const handleDelete = async (id: string) => {
    await unwrap(commands.deleteClip(id));
    if (selectedClip?.id === id) setSelectedClip(null);
    refreshClips();
    refreshSources();
    showToast('Deleted', 'trash');
  };

  const handlePin = async (clip: LocalClip, note: string | null) => {
    await unwrap(commands.pinClip(clip.id, note));
    setPinNoteDialog(null);
    refreshClips();
    showToast('Pinned', 'copy');
  };

  const handleUnpin = async (clip: LocalClip) => {
    await unwrap(commands.unpinClip(clip.id));
    refreshClips();
    showToast('Unpinned', 'trash');
  };

  const totalClips = sources.reduce((sum, s) => sum + s.clip_count, 0);

  // Build source -> nickname map for SourcePill and from: filter
  const nicknameBySource = useMemo(() => {
    const map: Record<string, string> = { ...displayNames };
    for (const d of devices) {
      if (d.nickname && d.source_key) {
        map[d.source_key] = displayNames[d.source_key] ?? d.nickname;
      }
    }
    return map;
  }, [displayNames, devices]);

  const deviceOptions = useMemo<DeviceOption[]>(
    () => buildDeviceOptions({ devices, sources, displayNames, tagColors }),
    [devices, sources, displayNames, tagColors],
  );

  // Use results from Rust directly — no more client-side fuzzy filtering!
  const navOrderClips = useMemo(() => {
    if (activePanel === 'pinned') return clips;
    return groupByTimeBucket(clips).flatMap((g) => g.items);
  }, [clips, activePanel]);

  // User-customizable clip-action shortcuts (Settings → Keyboard). Loaded once;
  // SettingsPane pushes edits back via onActionShortcutsChange so the live
  // handler below updates without a restart.
  const [actionShortcuts, setActionShortcuts] = useState<ActionShortcuts>(DEFAULT_ACTION_SHORTCUTS);
  useEffect(() => {
    unwrap(commands.getActionShortcuts())
      .then((s) => { if (s) setActionShortcuts(s); })
      .catch(() => {/* keep defaults */});
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Ignore IME composition keydowns (e.g. the Enter that commits a Korean
      // composition arrives as key "Process" / keyCode 229) so they never
      // swallow a shortcut. The next, real keydown is handled normally.
      if (isImeComposition(e)) return;

      // While a modal is open it owns the keyboard (each has its own Esc and,
      // for the edit modal, ⌘↵ handler). Suppress every global shortcut so
      // that, e.g., ⌘↵ to save does not also broadcast the selected clip via
      // sendClip, ⌘P does not pin it, and Esc does not deselect it underneath
      // the modal.
      if (editDialog || pinNoteDialog) return;

      const target = e.target as HTMLElement | null;
      const isTextEntry =
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        !!target?.isContentEditable;

      // Resolve the physical key (e.code) for letter/digit shortcuts so they
      // survive Korean IME (ㅓ on KeyJ) and non-QWERTY layouts.
      const key = physicalKey(e);

      if ((e.metaKey || e.ctrlKey) && key === 'F') {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
      if (key === '/' && !e.metaKey && !e.ctrlKey && !e.altKey && !isTextEntry) {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
        return;
      }
      if (key === 'Escape') {
        if (showShortcuts) { setShowShortcuts(false); return; }
        if (document.activeElement === searchRef.current) {
          searchRef.current?.blur();
        } else if (searchQuery) {
          setSearchQuery('');
        } else if (selectedClip) {
          setSelectedClip(null);
        }
      }
      if (key === '?' && !(e.target instanceof HTMLInputElement)) {
        e.preventDefault();
        setShowShortcuts(v => !v);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && key === ',') {
        e.preventDefault();
        setSettingsTab('general'); setShowSettings(v => !v);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && (key === '1' || key === '2' || key === '3')) {
        e.preventDefault();
        const panels: RailPanel[] = ['inbox', 'pinned', 'devices'];
        const panel = panels[parseInt(key) - 1];
        setActivePanel(panel);
        setSelectedClip(null);
        setSelectedSource(null);
        return;
      }
      if (
        key === 'Tab' &&
        !e.metaKey && !e.ctrlKey && !e.altKey &&
        !(e.target instanceof HTMLInputElement) &&
        !(e.target instanceof HTMLTextAreaElement)
      ) {
        e.preventDefault();
        const panels: RailPanel[] = ['inbox', 'pinned', 'devices'];
        const idx = panels.indexOf(activePanel);
        const next = e.shiftKey
          ? (idx - 1 + panels.length) % panels.length
          : (idx + 1) % panels.length;
        setActivePanel(panels[next]);
        setSelectedClip(null);
        setSelectedSource(null);
        return;
      }
      // Pin/unpin (default ⌘P): keep the unconditional preventDefault so the
      // default binding still suppresses the webview print dialog; act only
      // when a clip is selected.
      if (matchesAccelerator(e, actionShortcuts.pin)) {
        e.preventDefault();
        if (selectedClip) {
          if (selectedClip.is_pinned) {
            handleUnpin(selectedClip);
          } else {
            setPinNoteDialog({ clip: selectedClip });
          }
        }
        return;
      }
      // Edit (default ⌘E): text clips only, never while typing in a field.
      if (matchesAccelerator(e, actionShortcuts.edit) && !isTextEntry && selectedClip && selectedClip.content_type !== 'image') {
        e.preventDefault();
        setEditDialog({ clip: selectedClip });
        return;
      }
      if (selectedClip) {
        // Send (default ⌘↵) is checked before Copy (default ↵). Exact-modifier
        // matching already keeps them mutually exclusive, but the else-if
        // preserves the original "only one fires" intent.
        if (matchesAccelerator(e, actionShortcuts.send)) {
          e.preventDefault();
          void sendClip(selectedClip);
        } else if (matchesAccelerator(e, actionShortcuts.copy) && (!isTextEntry || e.target === searchRef.current)) {
          e.preventDefault();
          copyClip(selectedClip);
        }
        // ⌘C stays a fixed copy alias (only when no text is selected).
        if ((e.metaKey || e.ctrlKey) && key === 'C') {
          if (!window.getSelection()?.toString()) copyClip(selectedClip);
        }
      }
      // Ctrl+H / Ctrl+L — cycle sources (only when not typing in search)
      if (e.ctrlKey && (key === 'H' || key === 'L') && !(e.target instanceof HTMLInputElement)) {
        e.preventDefault();
        const all = [null, ...sources.map((s) => s.source)];
        const idx = all.indexOf(selectedSource);
        const next = key === 'L'
          ? (idx + 1) % all.length
          : (idx - 1 + all.length) % all.length;
        setSelectedSource(all[next]);
        setSelectedClip(null);
      }
      const isDown = key === 'ArrowDown' || (e.ctrlKey && key === 'J');
      const isUp = key === 'ArrowUp' || (e.ctrlKey && key === 'K');
      if (isDown || isUp) {
        if (navOrderClips.length === 0) return;
        e.preventDefault();
        const idx = selectedClip ? navOrderClips.findIndex((c) => c.id === selectedClip.id) : -1;
        const next = idx === -1
          ? 0
          : isDown
            ? Math.min(idx + 1, navOrderClips.length - 1)
            : Math.max(idx - 1, 0);
        setSelectedClip(navOrderClips[next]);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [searchQuery, selectedClip, navOrderClips, sources, selectedSource, copyClip, sendClip, showShortcuts, activePanel, editDialog, pinNoteDialog, actionShortcuts]);

  const currentDeviceID =
    auth.variant === 'Authenticated' ? auth.payload.device_id : '';
  const currentMachineId =
    auth.variant === 'Authenticated' ? auth.payload.machine_id : '';

  const handoffDialog = handoffRelay !== null ? (
    <AddRelayDialog
      onClose={() => setHandoffRelay(null)}
      initialRelayUrl={handoffRelay}
      fromCli
    />
  ) : null;

  if (showSettings) {
    return (
      <>
        <SettingsPane
          onClose={() => { setShowSettings(false); if (auth.variant === 'Authenticated') refreshDevices(); }}
          clipCount={totalClips}
          initialTab={settingsTab}
          onActionShortcutsChange={setActionShortcuts}
        />
        {handoffDialog}
      </>
    );
  }

  if (auth.variant === 'LocalOnly') {
    return (
      <>
        <OnboardingScreen
          onShowSettings={() => openSettings('privacy')}
          onMouseDown={handleWindowDrag}
        />
        {handoffRelay !== null && (
          <AddRelayDialog
            onClose={() => setHandoffRelay(null)}
            initialRelayUrl={handoffRelay}
            fromCli
          />
        )}
        <AdoptedAuthToast />
        <OfflineQueueDroppedToast />
        <ClipDecryptFailedToast />
        <SendToast />
      </>
    );
  }
  if (auth.variant === 'Authenticating') {
    return <AuthLoadingScreen progress={auth.payload.progress} />;
  }
  if (auth.variant === 'ErrorRecoverable') {
    return (
      <AuthErrorScreen
        reason={auth.payload.reason}
        retryAfterMs={auth.payload.retry_after_ms}
      />
    );
  }
  // auth.variant === 'Authenticated' — render dashboard.

  return (
    <main data-testid="dashboard-root" style={S.main}>
      <Rail
        active={activePanel}
        onSelect={(panel) => {
          setActivePanel(panel);
          setSelectedClip(null);
          setSelectedSource(null);
          setActiveFilter('all');
        }}
        onOpenSettings={() => openSettings()}
      />

      <div style={S.mainCol}>
        <UpdateBanner currentVersion={currentDesktopVersion} latest={latestVersions} />
        <SearchBar
          ref={searchRef}
          value={searchQuery}
          onChange={setSearchQuery}
          onClear={() => setSearchQuery('')}
          themeMode={themeMode}
          onSetThemeMode={setThemeMode}
          onMouseDown={handleWindowDrag}
          activeFilter={activeFilter}
          onFilterChange={setActiveFilter}
          deviceOptions={deviceOptions}
          selectedSource={selectedSource}
          onSourceChange={setSelectedSource}
        />

        <div style={S.body}>
        {activePanel === 'devices' ? (
          <DevicesPanel
            currentDeviceID={currentDeviceID}
            currentMachineId={currentMachineId}
            onShowToast={(msg) => showToast(msg, 'copy')}
            onDeviceChange={refreshDevices}
          />
        ) : activePanel === 'pinned' ? (
          <PinnedPanel
            clips={clips}
            selected={selectedClip}
            onSelect={setSelectedClip}
            onCopy={copyClip}
            onPin={(c) => setPinNoteDialog({ clip: c })}
            onUnpin={handleUnpin}
            onDelete={(c) => handleDelete(c.id)}
            onSaveImage={handleSaveImage}
            query={debouncedQuery}
            deviceNicknames={nicknameBySource}
            tagColors={tagColors}
            listRef={clipListRef}
            actionShortcuts={actionShortcuts}
          />
        ) : !clipsLoaded ? (
          <ClipListSkeleton />
        ) : clips.length === 0 && devices.length <= 1 ? (
          <GettingStartedCard
            onCopySnippet={(text) => {
              void unwrap(commands.copyClipToClipboard(text));
              showToast('Copied to clipboard', 'copy');
            }}
          />
        ) : (
          <>
            <ClipList
              ref={clipListRef}
              clips={clips}
              selected={selectedClip}
              onSelect={setSelectedClip}
              onCopy={copyClip}
              onSend={sendClip}
              query={debouncedQuery}
              deviceNicknames={nicknameBySource}
              tagColors={tagColors}
            />
            <ClipDetail
              clip={selectedClip}
              onCopy={copyClip}
              onPin={(c) => c.is_pinned ? handleUnpin(c) : setPinNoteDialog({ clip: c })}
              onDelete={(c) => handleDelete(c.id)}
              onSaveImage={handleSaveImage}
              onEdit={(c) => setEditDialog({ clip: c })}
              searchQuery={debouncedQuery}
              tagColors={tagColors}
              sourceDisplayNames={nicknameBySource}
              actionShortcuts={actionShortcuts}
            />
          </>
        )}
        </div>
      </div>

      {selectedClip && (
        <HiddenActions
          onCopy={() => copyClip(selectedClip)}
          onDelete={() => handleDelete(selectedClip.id)}
        />
      )}

      {pinNoteDialog && (
        <PinNoteDialog
          clip={pinNoteDialog.clip}
          onConfirm={(note) => handlePin(pinNoteDialog.clip, note || null)}
          onCancel={() => setPinNoteDialog(null)}
        />
      )}

      {editDialog && (
        <EditClipModal
          clip={editDialog.clip}
          onSave={(text) => handleEdit(editDialog.clip, text)}
          onCancel={() => setEditDialog(null)}
        />
      )}

      {newSourcePrompt && (
        <NewSourceDialog
          source={newSourcePrompt}
          onAccept={() => setNewSourcePrompt(null)}
          onDisableAutoCopy={() => handleNewSourceResponse(newSourcePrompt, false)}
        />
      )}

      {showShortcuts && <ShortcutPanel onClose={() => setShowShortcuts(false)} actionShortcuts={actionShortcuts} />}
      {toast && <Toast message={toast.message} icon={toast.icon} />}
      <AdoptedAuthToast />
      <OfflineQueueDroppedToast />
      <ClipDecryptFailedToast />
      <SendToast />
      {handoffDialog}
    </main>
  );
}

// ─── Auth transition screens (plumbing only per D-14 — no visual redesign) ────

function AuthLoadingScreen({ progress }: { progress: AuthProgress }) {
  const [timedOut, setTimedOut] = useState(false);
  const prefersReducedMotion =
    typeof window !== 'undefined' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  useEffect(() => {
    const timer = setTimeout(() => setTimedOut(true), 5 * 60 * 1000); // 5 minutes
    return () => clearTimeout(timer);
  }, []);

  const heading = timedOut
    ? 'Sign-in timed out.'
    : progress.kind === 'SigningIn'
      ? 'Signing in...'
      : progress.kind === 'Pairing'
        ? 'Pairing device...'
        : 'Rotating token...';

  const subtext = timedOut
    ? 'Try again when ready.'
    : 'Complete sign-in in your browser.';

  const buttonLabel = timedOut ? 'Back to local mode' : 'Stop sign-in';

  const handleCancel = async () => {
    try {
      await signOut();
    } catch (e) {
      console.error('cancel auth failed:', e);
    }
  };

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        height: '100vh',
        gap: 24,
        color: C.t1,
        background: C.bg,
        fontFamily: 'inherit',
      }}
    >
      {/* Spinner or static dot */}
      {prefersReducedMotion ? (
        <span
          style={{
            width: 20,
            height: 20,
            borderRadius: '50%',
            backgroundColor: C.t1,
          }}
        />
      ) : (
        <span
          style={{
            width: 20,
            height: 20,
            border: '2px solid transparent',
            borderTopColor: C.t1,
            borderRightColor: C.t1,
            borderBottomColor: C.t1,
            borderRadius: '50%',
            animation: 'spin 800ms linear infinite',
            boxSizing: 'border-box',
          }}
        />
      )}

      {/* Heading */}
      <span
        style={{
          fontSize: 22,
          fontWeight: 600,
          letterSpacing: '-0.018em',
          color: C.t1,
        }}
      >
        {heading}
      </span>

      {/* Subtext */}
      <span
        style={{
          fontFamily: 'var(--font-body)',
          fontSize: 14,
          fontWeight: 500,
          color: C.t2,
          marginTop: -16,
        }}
      >
        {subtext}
      </span>

      {/* Cancel button — ghost style */}
      <button
        onClick={handleCancel}
        style={{
          background: 'transparent',
          border: 'none',
          cursor: 'pointer',
          fontFamily: 'var(--font-body)',
          fontSize: 14,
          fontWeight: 500,
          color: C.t3,
          padding: '6px 14px',
          borderRadius: 4,
          transition: 'color 150ms ease',
        }}
        onMouseEnter={(e) => { (e.target as HTMLButtonElement).style.color = 'var(--text-primary)'; }}
        onMouseLeave={(e) => { (e.target as HTMLButtonElement).style.color = 'var(--text-faint)'; }}
      >
        {buttonLabel}
      </button>
    </div>
  );
}

function AuthErrorScreen({
  reason,
  retryAfterMs,
}: {
  reason: AuthErrorReason;
  retryAfterMs: number | null;
}) {
  const [retrying, setRetrying] = useState(false);
  const label =
    reason.kind === 'RelayUnreachable'
      ? 'Relay unreachable'
      : reason.kind === 'KeyringUnavailable'
        ? 'Keyring unavailable'
        : reason.kind === 'NetworkDown'
          ? 'No network connection'
          : 'Invalid pair token';
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        height: '100vh',
        gap: 16,
        color: C.t1,
        background: C.bg,
        fontFamily: 'inherit',
      }}
    >
      <span style={{ fontSize: 22, fontWeight: 600, letterSpacing: '-0.018em', color: C.t1 }}>{label}</span>
      {retryAfterMs !== null && (
        <span style={{ color: C.t3, fontSize: 14 }}>
          Auto-retry in {Math.round(retryAfterMs / 1000)}s
        </span>
      )}
      <button
        onClick={async () => {
          setRetrying(true);
          await retryAuth();
          setRetrying(false);
        }}
        disabled={retrying}
        style={dialogStyles.btnPrimary}
      >
        Retry now
      </button>
    </div>
  );
}

// ─── Internal helper components ────────────────────────────

// Off-screen buttons so screen-readers/keyboard can still trigger actions.
function HiddenActions({ onCopy, onDelete }: { onCopy: () => void; onDelete: () => void }) {
  return (
    <div style={{ position: 'absolute', left: -9999, top: -9999 }} aria-hidden="true">
      <button onClick={onCopy}><IconCopy /></button>
      <button onClick={onDelete}><IconTrash /></button>
    </div>
  );
}

function ShortcutPanel({ onClose, actionShortcuts }: { onClose: () => void; actionShortcuts: ActionShortcuts }) {
  const groups: { title: string; rows: { keys: string[]; label: string }[] }[] = [
    {
      title: 'Navigation',
      rows: [
        { keys: ['↑', '↓'], label: 'Move between clips' },
        { keys: ['^J', '^K'], label: 'Move between clips (vim)' },
        { keys: ['^H', '^L'], label: 'Cycle source filter' },
        { keys: ['⌘1'], label: 'Go to Inbox' },
        { keys: ['⌘2'], label: 'Go to Pinned' },
        { keys: ['⌘3'], label: 'Go to Devices' },
        { keys: ['⇥'], label: 'Next panel (Inbox → Pinned → Devices)' },
        { keys: ['⇧⇥'], label: 'Previous panel' },
      ],
    },
    {
      // Action keys mirror the user's configured clip-action shortcuts
      // (Settings → Keyboard → Clip actions). ⌘C stays a fixed copy alias.
      title: 'Actions',
      rows: [
        { keys: [formatShortcutDisplay(actionShortcuts.copy)], label: 'Copy selected clip' },
        { keys: ['⌘C'], label: 'Copy selected clip' },
        { keys: [formatShortcutDisplay(actionShortcuts.pin)], label: 'Pin / unpin selected clip' },
        { keys: [formatShortcutDisplay(actionShortcuts.edit)], label: 'Edit selected clip' },
        { keys: [formatShortcutDisplay(actionShortcuts.send)], label: 'Send / broadcast selected clip' },
      ],
    },
    {
      title: 'Search',
      rows: [
        { keys: ['⌘F', '/'], label: 'Focus search' },
        { keys: ['Esc'], label: 'Clear search / deselect' },
      ],
    },
    {
      title: 'General',
      rows: [
        { keys: ['?'], label: 'Toggle this panel' },
        { keys: ['⌘,'], label: 'Open settings' },
      ],
    },
  ];

  const kbdStyle: React.CSSProperties = {
    fontFamily: 'var(--font-mono)',
    fontSize: 10,
    padding: '1px 5px',
    background: 'var(--kbd-bg)',
    border: '1px solid var(--kbd-border)',
    borderRadius: 3,
    color: 'var(--kbd-color)',
    lineHeight: 1.4,
    minWidth: 16,
    textAlign: 'center',
  };

  return (
    <div style={dialogStyles.overlay} onClick={onClose}>
      <div style={{ ...dialogStyles.dialog, maxWidth: 340, padding: 20 }} onClick={(e) => e.stopPropagation()}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 16 }}>
          <span style={{ fontSize: 13, fontWeight: 600, color: C.t1 }}>Keyboard shortcuts</span>
          <button style={{ ...dialogStyles.btnGhost, padding: '2px 8px', fontSize: 11 }} onClick={onClose}>Esc</button>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          {groups.map((g) => (
            <div key={g.title}>
              <div style={{ fontSize: 11, fontWeight: 600, color: C.t3, letterSpacing: '0.01em', marginBottom: 8 }}>
                {g.title}
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
                {g.rows.map((r) => (
                  <div key={r.label} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                    <span style={{ fontSize: 12, color: C.t2 }}>{r.label}</span>
                    <div style={{ display: 'flex', gap: 4 }}>
                      {r.keys.map((k) => (
                        <kbd key={k} style={kbdStyle}>{k}</kbd>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function PinNoteDialog({
  clip,
  onConfirm,
  onCancel,
}: {
  clip: LocalClip;
  onConfirm: (note: string) => void;
  onCancel: () => void;
}) {
  const [note, setNote] = useState(clip.pin_note ?? '');
  const preview = clip.content.replace(/\s+/g, ' ').trim().substring(0, 60);

  return (
    <div style={dialogStyles.overlay} onClick={onCancel}>
      <div style={{ ...dialogStyles.dialog, maxWidth: 360 }} onClick={(e) => e.stopPropagation()}>
        <div style={dialogStyles.title}>Pin clip</div>
        <div style={{ fontSize: 11, color: C.t3, marginBottom: 10, fontFamily: 'var(--font-mono)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {preview || '(image)'}
        </div>
        <textarea
          autoFocus
          placeholder="Add a note (optional)"
          value={note}
          onChange={(e) => setNote(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); onConfirm(note); }
            if (e.key === 'Escape') onCancel();
          }}
          style={{
            width: '100%',
            minHeight: 60,
            background: C.card2,
            border: `1px solid ${C.border}`,
            borderRadius: 4,
            color: C.t1,
            fontSize: 12,
            fontFamily: 'inherit',
            padding: '6px 8px',
            resize: 'none',
            outline: 'none',
            boxSizing: 'border-box',
            marginBottom: 12,
          }}
        />
        <div style={dialogStyles.actions}>
          <button style={dialogStyles.btnGhost} onClick={onCancel}>Cancel</button>
          <button style={dialogStyles.btnPrimary} onClick={() => onConfirm(note)}>Pin</button>
        </div>
      </div>
    </div>
  );
}

function Toast({ message, icon }: { message: string; icon: 'copy' | 'trash' | 'error' }) {
  const toastStyle: React.CSSProperties = {
    position: 'fixed',
    bottom: 44,
    left: '50%',
    transform: 'translateX(-50%)',
    background: C.card2,
    border: `1px solid ${C.border}`,
    borderRadius: 8,
    padding: '6px 14px',
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    zIndex: 200,
    pointerEvents: 'none',
    boxShadow: '0 4px 20px rgba(10, 8, 5, 0.4)',
    whiteSpace: 'nowrap',
  };
  const textStyle: React.CSSProperties = { fontSize: 12, color: C.t2 };
  return (
    <div style={toastStyle}>
      <span style={{ color: C.t3, display: 'flex', alignItems: 'center' }}>
        {icon === 'copy' ? (
          <IconCopy size={12} />
        ) : icon === 'error' ? (
          <IconX size={12} />
        ) : (
          <IconTrash size={12} />
        )}
      </span>
      <span style={textStyle}>{message}</span>
    </div>
  );
}

function NewSourceDialog({
  source,
  onAccept,
  onDisableAutoCopy,
}: {
  source: string;
  onAccept: () => void;
  onDisableAutoCopy: () => void;
}) {
  return (
    <div style={dialogStyles.overlay} onClick={onAccept}>
      <div style={dialogStyles.dialog} onClick={(e) => e.stopPropagation()}>
        <div style={dialogStyles.title}>New source detected</div>
        <div style={dialogStyles.body}>
          <code style={{ color: C.t1, fontFamily: 'var(--font-mono)' }}>
            {source.replace('remote:', '')}
          </code>{' '}
          is sending clips. Auto-copy is on by default.
        </div>
        <div style={dialogStyles.actions}>
          <button style={dialogStyles.btnGhost} onClick={onDisableAutoCopy}>
            Disable auto-copy
          </button>
          <button style={dialogStyles.btnPrimary} onClick={onAccept}>OK</button>
        </div>
      </div>
    </div>
  );
}

// ─── Styles ────────────────────────────────────────────────

const S: Record<string, React.CSSProperties> = {
  main: {
    background: C.bg,
    color: C.t1,
    height: '100vh',
    display: 'flex',
    flexDirection: 'row',
    position: 'relative',
    borderRadius: 'var(--radius-xl)',
    overflow: 'hidden',
    border: `1px solid ${C.border}`,
  },
  // Right column: search toolbar + panes, sitting to the right of the
  // full-height rail (the rail is now the leftmost full-height element).
  mainCol: {
    display: 'flex',
    flexDirection: 'column',
    flex: 1,
    minWidth: 0,
    minHeight: 0,
  },
  body: {
    display: 'flex',
    flex: 1,
    minHeight: 0,
    overflow: 'hidden',
  },

};

export default App;
