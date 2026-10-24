import { useEffect, useLayoutEffect, useState, useCallback, useRef, useMemo } from 'react';
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
import ConfirmDialog from './ConfirmDialog';
import { AdoptedAuthToast } from './components/AdoptedAuthToast';
import { OfflineQueueDroppedToast } from './components/OfflineQueueDroppedToast';
import { ClipDecryptFailedToast } from './components/ClipDecryptFailedToast';
import { SendToast } from './components/SendToast';
import { AddRelayDialog } from './components/AddRelayDialog';
import { SearchBar, type DeviceOption, type AppOption } from './components/SearchBar';
import { buildDeviceOptions } from './lib/deviceOptions';
import { ClipList } from './components/ClipList';
import { ClipListSkeleton } from './components/ClipListSkeleton';
import { ClipDetail } from './components/ClipDetail';
import { EditClipModal } from './components/EditClipModal';
import { DevicesPanel } from './components/DevicesPanel';
import { GettingStartedCard } from './components/GettingStartedCard';
import { OnboardingScreen } from './components/OnboardingScreen';
import { dialogStyles } from './components/dialogPrimitives';
import { IconCopy, IconTrash, IconX, IconGear } from './icons';
import { UpdateBanner } from './components/UpdateBanner';
import { useLatestVersions } from './lib/state/versions';
import packageJson from '../package.json';
import './App.css';

function handleWindowDrag(e: React.MouseEvent) {
  const target = e.target as HTMLElement;
  if (!target.closest('button, input, a, textarea, select, [role="menu"], [role="listbox"]')) {
    void commands.snapDragStart();
    void getCurrentWindow().startDragging();
  }
}

const WINDOW_PRESETS = {
  compact:  { width: 760,  height: 480 },
  standard: { width: 960,  height: 600 },
  spacious: { width: 1120, height: 720 },
} as const;

type Panel = 'inbox' | 'pinned' | 'devices';
interface HistorySnapshot {
  query: string;
  source: string | null;
  app: string | null;
  filter: ClipFilter;
  selectedId: string | null;
  scrollTop: number;
}
const PINNED_SHORTCUT = 'CmdOrCtrl+Shift+P';
const ACTIONS_SHORTCUT = 'CmdOrCtrl+Shift+A';

function App() {
  useTheme();

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
    if (!notifyOnRemoteLogin || auth.variant !== 'Authenticated') return;
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
  }, [notifyOnRemoteLogin, auth.variant]);

  const [_status, setStatus] = useState('connecting');
  const [clips, setClips] = useState<LocalClip[]>([]);
  // False until the first clip fetch settles, so the inbox shows a loading
  // skeleton instead of flashing the empty state before clips arrive.
  const [clipsLoaded, setClipsLoaded] = useState(false);
  const [sources, setSources] = useState<SourceInfo[]>([]);
  const [selectedClip, setSelectedClip] = useState<LocalClip | null>(null);
  const [selectedSource, setSelectedSource] = useState<string | null>(null);
  const [selectedApp, setSelectedApp] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [devices, setDevices] = useState<Device[]>([]);
  const [appOptions, setAppOptions] = useState<AppOption[]>([]);
  const { tagColors, displayNames } = useMachineLabels();
  const [newSourcePrompt, setNewSourcePrompt] = useState<string | null>(null);
  const [pinNoteDialog, setPinNoteDialog] = useState<{ clip: LocalClip } | null>(null);
  const [editDialog, setEditDialog] = useState<{ clip: LocalClip } | null>(null);
  const [deleteDialog, setDeleteDialog] = useState<{ clip: LocalClip } | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>('general');
  const openSettings = (tab: SettingsTab = 'general') => {
    setOpenMenu(null);
    setSettingsTab(tab);
    setShowSettings(true);
  };
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [activePanel, setActivePanel] = useState<Panel>('inbox');
  const [activeFilter, setActiveFilter] = useState<ClipFilter>('all');
  const searchRef = useRef<HTMLInputElement>(null);
  const clipListRef = useRef<HTMLDivElement>(null);
  const copyRecencyRef = useRef<Map<string, number>>(new Map());
  const historySnapshotRef = useRef<HistorySnapshot | null>(null);
  const restoreSelectionRef = useRef<string | null>(null);
  const restoreScrollRef = useRef<number | null>(null);
  const skipSelectionScrollRef = useRef(false);
  const clipRequestRef = useRef(0);
  const clipViewRef = useRef({ activePanel, selectedSource, selectedApp, debouncedQuery, activeFilter });
  const [openMenu, setOpenMenu] = useState<'actions' | 'view' | null>(null);
  const actionsRef = useRef<HTMLDivElement>(null);
  const actionsTriggerRef = useRef<HTMLButtonElement>(null);
  const viewRef = useRef<HTMLDivElement>(null);
  const viewTriggerRef = useRef<HTMLButtonElement>(null);
  const [filterRequest, setFilterRequest] = useState<'device' | 'app' | null>(null);
  const canUseClipActions = selectedClip !== null && activePanel !== 'devices';

  const closeMenu = useCallback(() => {
    setOpenMenu(null);
    (openMenu === 'view' ? viewTriggerRef : actionsTriggerRef).current?.focus();
  }, [openMenu]);

  const showPanel = useCallback((panel: Panel) => {
    setOpenMenu(null);
    if (panel === activePanel) return;
    if (activePanel === 'inbox') {
      historySnapshotRef.current = {
        query: searchQuery, source: selectedSource, app: selectedApp,
        filter: activeFilter, selectedId: selectedClip?.id ?? null,
        scrollTop: clipListRef.current?.scrollTop ?? 0,
      };
    }
    const snapshot = panel === 'inbox' ? historySnapshotRef.current : null;
    ++clipRequestRef.current;
    setActivePanel(panel);
    setSearchQuery(snapshot?.query ?? '');
    setDebouncedQuery(snapshot?.query ?? '');
    setSelectedSource(snapshot?.source ?? null);
    setSelectedApp(snapshot?.app ?? null);
    setActiveFilter(snapshot?.filter ?? 'all');
    setSelectedClip(null);
    setClips([]);
    setClipsLoaded(false);
    restoreSelectionRef.current = snapshot?.selectedId ?? null;
    restoreScrollRef.current = snapshot?.scrollTop ?? 0;
    skipSelectionScrollRef.current = snapshot !== null;
  }, [activePanel, searchQuery, selectedSource, selectedApp, activeFilter, selectedClip]);

  const resetPinnedView = useCallback(() => {
    if (clipViewRef.current.activePanel !== 'pinned') return;
    ++clipRequestRef.current;
    setActivePanel('inbox');
    setSelectedSource(null);
    setSelectedApp(null);
    setActiveFilter('all');
    setClips([]);
    setClipsLoaded(false);
    setOpenMenu(null);
    historySnapshotRef.current = null;
    restoreSelectionRef.current = null;
    restoreScrollRef.current = 0;
    skipSelectionScrollRef.current = false;
  }, []);

  useEffect(() => {
    if (activePanel !== 'devices') searchRef.current?.focus();
  }, [activePanel]);

  useEffect(() => {
    if (!openMenu) return;
    const container = openMenu === 'view' ? viewRef : actionsRef;
    container.current?.querySelector<HTMLButtonElement>('[role="menu"] button:not(:disabled)')?.focus();
    const outside = (e: MouseEvent) => {
      if (e.target instanceof Node && !container.current?.contains(e.target)) setOpenMenu(null);
    };
    document.addEventListener('mousedown', outside);
    return () => document.removeEventListener('mousedown', outside);
  }, [openMenu]);

  useEffect(() => {
    if (canUseClipActions) return;
    if (openMenu === 'actions') {
      setOpenMenu(null);
      searchRef.current?.focus();
    } else if (document.activeElement === document.body || document.activeElement === actionsTriggerRef.current) {
      searchRef.current?.focus();
    }
  }, [openMenu, canUseClipActions, clips.length]);

  useLayoutEffect(() => {
    clipViewRef.current = { activePanel, selectedSource, selectedApp, debouncedQuery, activeFilter };
    ++clipRequestRef.current;
    return () => { ++clipRequestRef.current; };
  }, [activePanel, selectedSource, selectedApp, debouncedQuery, activeFilter]);

  useLayoutEffect(() => {
    if (clipsLoaded && clipListRef.current && restoreScrollRef.current !== null) {
      clipListRef.current.scrollTop = restoreScrollRef.current;
      restoreScrollRef.current = null;
    }
  }, [clips, clipsLoaded]);
  // Tracks whether a clip-action modal was open on the previous render, so the
  // effect below can detect the open→closed edge and restore keyboard focus.
  const modalWasOpenRef = useRef(false);

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
    const { activePanel, selectedSource, selectedApp, debouncedQuery, activeFilter } = clipViewRef.current;
    if (activePanel === 'devices') return;
    const request = ++clipRequestRef.current;
    try {
      let finalQuery = debouncedQuery;
      if (activePanel === 'pinned') finalQuery = `is:pinned ${finalQuery}`.trim();
      if (selectedSource) finalQuery = `from:${selectedSource} ${finalQuery}`.trim();
      if (selectedApp) finalQuery = `app:${selectedApp} ${finalQuery}`.trim();
      if (activeFilter !== 'all') finalQuery = `type:${activeFilter} ${finalQuery}`.trim();
      const results = await unwrap(commands.listClips(finalQuery, 500));
      if (request !== clipRequestRef.current) return;
      const next = applyInboxRecency(results);
      const restoreId = restoreSelectionRef.current;
      restoreSelectionRef.current = null;
      setClips(next);
      setSelectedClip((current) => next.find((clip) => clip.id === (restoreId ?? current?.id)) ?? next[0] ?? null);
    } catch (e) {
      if (request === clipRequestRef.current) showToast('Could not load history. Try searching again.', 'error');
      console.error('failed to load clips:', e);
    } finally {
      if (request === clipRequestRef.current) setClipsLoaded(true);
    }
  }, [applyInboxRecency, showToast]);

  const refreshSources = useCallback(async () => {
    try {
      setSources(await unwrap(commands.getSources()));
    } catch (e) {
      console.error(e);
    }
  }, []);

  const refreshDevices = useCallback(async () => {
    if (auth.variant !== 'Authenticated') return;
    try {
      setDevices(await unwrap(commands.listDevices()));
    } catch (e) {
      console.error(e);
    }
  }, [auth.variant]);

  const refreshAppOptions = useCallback(async () => {
    try {
      const apps = await unwrap(commands.listSourceApps());
      setAppOptions(apps.map((a) => ({ id: a.app_id, label: a.app_name, count: a.count })));
    } catch (e) {
      console.error(e);
    }
  }, []);

  const handleNewSourceResponse = async (source: string, enable: boolean) => {
    await unwrap(commands.setSourceAutoCopy(source, enable));
    setNewSourcePrompt(null);
  };

  useEffect(() => {
    if (auth.variant !== 'Authenticated') {
      setDevices([]);
      return;
    }
    const timer = setTimeout(() => {
      refreshClips();
      refreshSources();
      refreshDevices();
      refreshAppOptions();
    }, 1000);
    return () => clearTimeout(timer);
  }, [auth.variant, refreshClips, refreshSources, refreshDevices, refreshAppOptions]);

  useEffect(() => { void refreshClips(); }, [refreshClips, activePanel, selectedSource, selectedApp, debouncedQuery, activeFilter]);
  useEffect(() => {
    void refreshSources();
    void refreshAppOptions();
  }, [refreshSources, refreshAppOptions]);

  // Scroll selected clip into view when navigating with keyboard
  useEffect(() => {
    if (!selectedClip || !clipListRef.current) return;
    if (skipSelectionScrollRef.current) { skipSelectionScrollRef.current = false; return; }
    const el = clipListRef.current.querySelector<HTMLElement>(`[data-id="${selectedClip.id}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  }, [selectedClip?.id]);

  // Return keyboard focus to the selected clip's row when a clip-action modal
  // (pin / edit / delete) closes. Those modals autofocus an input; unmounting
  // it on close would otherwise drop DOM focus to <body>, making the detail
  // panel feel "focused out" even though the clip stays selected. Refocusing
  // the row keeps the selection as the keyboard context.
  const modalOpen = Boolean(pinNoteDialog || editDialog || deleteDialog);
  useEffect(() => {
    if (modalWasOpenRef.current && !modalOpen && selectedClip && clipListRef.current) {
      clipListRef.current
        .querySelector<HTMLElement>(`[data-id="${selectedClip.id}"]`)
        ?.focus({ preventScroll: true });
    }
    modalWasOpenRef.current = modalOpen;
  }, [modalOpen, selectedClip]);

  useEffect(() => {
    commands.getWsStatus().then(setStatus).catch(() => {});
    const unsubs = [
      events.wsStatus.listen((e) => setStatus(e.payload)),
      events.clipReceived.listen(() => { refreshClips(); refreshSources(); refreshAppOptions(); }),
      events.remoteClipReceived.listen(() => { refreshClips(); refreshSources(); refreshAppOptions(); }),
      events.clipDeleted.listen(() => { refreshClips(); refreshSources(); refreshAppOptions(); }),
      events.devicesChanged.listen(() => { refreshDevices(); }),
      events.clipPinned.listen(() => { refreshClips(); }),
      events.newSourceDetected.listen((e) => {
        setNewSourcePrompt(e.payload);
      }),
      events.trayOpenSettings.listen(() => openSettings()),
    ];
    return () => { unsubs.forEach((p) => p.then((f) => f())); };
  }, [refreshClips, refreshSources, refreshDevices, refreshAppOptions]);

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
      resetPinnedView();
      setSelectedClip(null);
      setSearchQuery('');
      setDebouncedQuery('');
    });
    return () => { unsubBlur.then((f) => f()); };
  }, [resetPinnedView]);

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
    resetPinnedView();
    setSearchQuery('');
    setDebouncedQuery('');
    setSelectedClip(null);
    void refreshClips();
    void commands.focusPreviousApp();
  }, [bumpClipRecency, refreshClips, showToast, resetPinnedView]);

  const copyClip = useCallback(async (clip: LocalClip) => {
    try {
      if (clip.content_type === 'image') {
        await unwrap(commands.copyImageToClipboard(clip.id));
        finishCopy(clip, 'Copied image to clipboard');
      } else {
        await unwrap(commands.copyClipToClipboard(clip.content));
        finishCopy(clip, 'Copied text to clipboard');
      }
    } catch {
      showToast('Copy failed. Try again.', 'error');
    }
  }, [finishCopy, showToast]);

  // Broadcasts the clip to all of the user's devices.
  const sendClip = useCallback(async (clip: LocalClip) => {
    if (auth.variant !== 'Authenticated') {
      showToast('Connect a relay to send clips. Local copy is always available.', 'error');
      return;
    }
    try {
      await unwrap(commands.sendClip(clip.id));
      refreshClips();
      showToast('Sent', 'copy');
    } catch (e) {
      console.error('sendClip failed', e);
      showToast(e instanceof Error ? e.message : 'Send failed', 'error');
    }
  }, [auth.variant, refreshClips, showToast]);

  const handleEdit = useCallback(async (clip: LocalClip, newContent: string) => {
    try {
      const newClip = await unwrap(commands.editClip(clip.id, newContent));
      // Refresh the list and select the new clip BEFORE closing the modal.
      // The modal-close focus restoration (see the effect above) targets
      // `selectedClip`'s row, so the selection must already point at the edited
      // clip — and that row must already be in the list — or focus (and the
      // keyboard copy/send context) lands on the stale original clip.
      await refreshClips();
      setSelectedClip(newClip);
      setEditDialog(null);
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
    try {
      await unwrap(commands.deleteClip(id));
      if (selectedClip?.id === id) setSelectedClip(null);
      refreshClips();
      refreshSources();
      showToast('Deleted locally', 'trash');
    } catch {
      showToast('Delete failed. Try again.', 'error');
    }
  };

  const handlePin = async (clip: LocalClip, note: string | null) => {
    try {
      await unwrap(commands.pinClip(clip.id, note));
      refreshClips();
      showToast('Pinned locally', 'copy');
    } catch {
      showToast('Pin failed. Try again.', 'error');
    } finally {
      setPinNoteDialog(null);
    }
  };

  const handleUnpin = async (clip: LocalClip) => {
    try {
      await unwrap(commands.unpinClip(clip.id));
      refreshClips();
      showToast('Unpinned locally', 'trash');
    } catch {
      showToast('Unpin failed. Try again.', 'error');
    }
  };

  const totalClips = sources.reduce((sum, s) => sum + s.clip_count, 0);

  // Machine labels are shared by history metadata and source filters.
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

  // Keyboard order follows the visible time groups.
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
      if (isImeComposition(e) || e.defaultPrevented) return;
      if (editDialog || pinNoteDialog || deleteDialog || showSettings || handoffRelay !== null || newSourcePrompt) return;
      if (document.querySelector('[aria-modal="true"]')) return;
      const key = physicalKey(e);
      if (showShortcuts) {
        if (key === 'Escape') setShowShortcuts(false);
        return;
      }
      const target = e.target as HTMLElement | null;
      const isTextEntry = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement || !!target?.isContentEditable;
      const actionMatch = Object.values(actionShortcuts).some((shortcut) => matchesAccelerator(e, shortcut));
      if (!actionMatch && matchesAccelerator(e, PINNED_SHORTCUT)) {
        e.preventDefault();
        showPanel(activePanel === 'pinned' ? 'inbox' : 'pinned');
        return;
      }
      if (!actionMatch && matchesAccelerator(e, ACTIONS_SHORTCUT)) {
        e.preventDefault();
        if (openMenu === 'actions') closeMenu();
        else if (canUseClipActions) setOpenMenu('actions');
        return;
      }
      if (openMenu) {
        if (key === 'Escape') { e.preventDefault(); closeMenu(); }
        return;
      }
      if (key === 'Escape') {
        e.preventDefault();
        if (activePanel !== 'inbox') showPanel('inbox');
        else if (searchQuery) setSearchQuery('');
        else searchRef.current?.blur();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && key === 'F' && !actionMatch) {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
        return;
      }
      if (key === '/' && !e.metaKey && !e.ctrlKey && !e.altKey && !isTextEntry) {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
        return;
      }
      if (key === '?' && !isTextEntry) {
        e.preventDefault();
        setShowShortcuts(true);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && key === ',') {
        e.preventDefault();
        setSettingsTab('general'); setShowSettings(true);
        return;
      }
      if (!actionMatch && (e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && ['1', '2', '3'].includes(key)) {
        e.preventDefault();
        showPanel((['inbox', 'pinned', 'devices'] as const)[Number(key) - 1]);
        return;
      }
      if (activePanel === 'devices') return;
      if (matchesAccelerator(e, actionShortcuts.pin)) {
        e.preventDefault();
        if (selectedClip) {
          if (selectedClip.is_pinned) void handleUnpin(selectedClip);
          else setPinNoteDialog({ clip: selectedClip });
        }
        return;
      }
      if (matchesAccelerator(e, actionShortcuts.edit) && !isTextEntry && selectedClip && selectedClip.content_type !== 'image') {
        e.preventDefault();
        setEditDialog({ clip: selectedClip });
        return;
      }
      if ((e.metaKey || e.ctrlKey) && key === 'Backspace' && !isTextEntry) {
        e.preventDefault();
        if (selectedClip) setDeleteDialog({ clip: selectedClip });
        return;
      }
      if (matchesAccelerator(e, actionShortcuts.save) && !isTextEntry && selectedClip?.content_type === 'image') {
        e.preventDefault();
        void handleSaveImage(selectedClip);
        return;
      }
      if (selectedClip && matchesAccelerator(e, actionShortcuts.send)) {
        e.preventDefault();
        void sendClip(selectedClip);
        return;
      }
      if (selectedClip && matchesAccelerator(e, actionShortcuts.copy) && (!isTextEntry || e.target === searchRef.current)) {
        e.preventDefault();
        void copyClip(selectedClip);
        return;
      }
      const searchHasSelection = target === searchRef.current && target instanceof HTMLInputElement &&
        target.selectionStart !== target.selectionEnd;
      if (selectedClip && (e.metaKey || e.ctrlKey) && key === 'C' &&
          (!isTextEntry || target === searchRef.current) && !searchHasSelection && !window.getSelection()?.toString()) {
        e.preventDefault();
        void copyClip(selectedClip);
        return;
      }
      if (e.ctrlKey && (key === 'H' || key === 'L') && !isTextEntry) {
        e.preventDefault();
        const all = [null, ...sources.map((source) => source.source)];
        const idx = all.indexOf(selectedSource);
        setSelectedSource(all[key === 'L' ? (idx + 1) % all.length : (idx - 1 + all.length) % all.length]);
        setSelectedClip(null);
      }
      const isDown = key === 'ArrowDown' || (e.ctrlKey && key === 'J');
      const isUp = key === 'ArrowUp' || (e.ctrlKey && key === 'K');
      if ((isDown || isUp) && (!isTextEntry || target === searchRef.current)) {
        if (!navOrderClips.length) return;
        e.preventDefault();
        const idx = selectedClip ? navOrderClips.findIndex((clip) => clip.id === selectedClip.id) : -1;
        const next = idx === -1 ? 0 : isDown ? Math.min(idx + 1, navOrderClips.length - 1) : Math.max(idx - 1, 0);
        setSelectedClip(navOrderClips[next]);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [searchQuery, selectedClip, navOrderClips, sources, selectedSource, copyClip, sendClip, handleSaveImage,
    showShortcuts, activePanel, editDialog, pinNoteDialog, deleteDialog, showSettings, handoffRelay,
    newSourcePrompt, actionShortcuts, openMenu, closeMenu, canUseClipActions, showPanel]);

  const runAction = (action: () => void) => { closeMenu(); action(); };
  const openFilter = (mode: 'device' | 'app') => {
    setFilterRequest(mode);
    searchRef.current?.focus();
  };
  const menuKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape' || e.key === 'Tab') {
      e.preventDefault(); e.stopPropagation(); closeMenu(); return;
    }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    e.preventDefault(); e.stopPropagation();
    const items = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
    const index = items.findIndex((item) => item === document.activeElement);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 :
      (index + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  };
  const pinnedShortcutAvailable = !Object.values(actionShortcuts).some((shortcut) =>
    formatShortcutDisplay(shortcut) === formatShortcutDisplay(PINNED_SHORTCUT));
  const actionsShortcutAvailable = !Object.values(actionShortcuts).some((shortcut) =>
    formatShortcutDisplay(shortcut) === formatShortcutDisplay(ACTIONS_SHORTCUT));

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

  return (
    <main data-testid="dashboard-root" style={S.main}>
      <div style={S.mainCol}>
        {auth.variant === 'Authenticating' && <AuthLoadingNotice progress={auth.payload.progress} />}
        {auth.variant === 'ErrorRecoverable' && <AuthErrorNotice reason={auth.payload.reason} />}
        <UpdateBanner currentVersion={currentDesktopVersion} latest={latestVersions} />
        {activePanel === 'devices' ? (
          <div className="history-toolbar" onMouseDown={handleWindowDrag}>
            <button type="button" onClick={() => showPanel('inbox')}>← Back to history</button>
          </div>
        ) : (
          <SearchBar
            key={activePanel}
            ref={searchRef}
            value={searchQuery}
            onChange={setSearchQuery}
            onClear={() => setSearchQuery('')}
            pinnedOnly={activePanel === 'pinned'}
            onLeavePinned={() => showPanel('inbox')}
            filterRequest={filterRequest}
            onFilterRequestHandled={() => setFilterRequest(null)}
            onMouseDown={handleWindowDrag}
            activeFilter={activeFilter}
            onFilterChange={setActiveFilter}
            deviceOptions={deviceOptions}
            selectedSource={selectedSource}
            onSourceChange={setSelectedSource}
            appOptions={appOptions}
            selectedApp={selectedApp}
            onAppChange={setSelectedApp}
            viewMenu={
              <div className="history-view" ref={viewRef}>
                <button type="button" ref={viewTriggerRef} aria-haspopup="menu" aria-expanded={openMenu === 'view'}
                  aria-controls="history-view-menu" onClick={() => setOpenMenu((menu) => menu === 'view' ? null : 'view')}>
                  View <span aria-hidden="true">⌄</span>
                </button>
                {openMenu === 'view' && (
                  <div id="history-view-menu" className="history-menu history-view-menu" role="menu" aria-label="View" tabIndex={-1} onKeyDown={menuKeyDown}>
                    <button type="button" role="menuitem" onClick={() => runAction(() => showPanel(activePanel === 'pinned' ? 'inbox' : 'pinned'))}>
                      {activePanel === 'pinned' ? 'Show recent history' : 'Show pinned'}
                      {pinnedShortcutAvailable && <kbd>{formatShortcutDisplay(PINNED_SHORTCUT)}</kbd>}
                    </button>
                    <div role="separator" />
                    <button type="button" role="menuitem" onClick={() => runAction(() => openFilter('device'))}>Filter by device <kbd>@</kbd></button>
                    <button type="button" role="menuitem" onClick={() => runAction(() => openFilter('app'))}>Filter by app <kbd>&gt;</kbd></button>
                  </div>
                )}
              </div>
            }
          />
        )}

        <div className="history-body" style={S.body}>
          {activePanel === 'devices' && auth.variant !== 'Authenticated' ? (
            <OnboardingScreen onShowSettings={() => openSettings('privacy')} connecting={auth.variant === 'Authenticating'} />
          ) : activePanel === 'devices' ? (
            <DevicesPanel currentDeviceID={currentDeviceID} currentMachineId={currentMachineId}
              onShowToast={(msg) => showToast(msg, 'copy')} onDeviceChange={refreshDevices} />
          ) : !clipsLoaded ? (
            <ClipListSkeleton />
          ) : activePanel === 'inbox' && clips.length === 0 && !debouncedQuery && !selectedSource && !selectedApp && activeFilter === 'all' ? (
            <GettingStartedCard onConnectRelay={() => showPanel('devices')} />
          ) : (
            <>
              <ClipList ref={clipListRef} clips={clips} selected={selectedClip}
                onSelect={setSelectedClip} onCopy={copyClip} query={debouncedQuery}
                pinnedOnly={activePanel === 'pinned'} deviceNicknames={nicknameBySource} />
              <ClipDetail clip={selectedClip} searchQuery={debouncedQuery} sourceDisplayNames={nicknameBySource} />
            </>
          )}
        </div>

        <footer className="history-footer">
          <div className="history-app-controls">
            <button type="button" onClick={() => openSettings()} title={formatShortcutDisplay('CmdOrCtrl+,')}>
              <IconGear /> Settings
            </button>
            {auth.variant !== 'Authenticated' && <span className="history-status">Local only</span>}
          </div>
          {activePanel !== 'devices' && (
            <button type="button" disabled={!selectedClip} onClick={() => selectedClip && void copyClip(selectedClip)}>
              Copy <kbd>{formatShortcutDisplay(actionShortcuts.copy)}</kbd>
            </button>
          )}
          <div className="history-actions" ref={actionsRef}>
            <button type="button" ref={actionsTriggerRef} disabled={!canUseClipActions}
              aria-haspopup="menu" aria-expanded={openMenu === 'actions' && canUseClipActions}
              aria-controls="history-actions-menu" onClick={() => setOpenMenu((menu) => menu === 'actions' ? null : 'actions')}>
              Actions {actionsShortcutAvailable && <kbd>{formatShortcutDisplay(ACTIONS_SHORTCUT)}</kbd>}
            </button>
            {openMenu === 'actions' && selectedClip && canUseClipActions && (
              <div id="history-actions-menu" className="history-menu" role="menu" aria-label="Clip actions" tabIndex={-1} onKeyDown={menuKeyDown}>
                <button type="button" role="menuitem" onClick={() => runAction(() => void copyClip(selectedClip))}>
                  Copy <kbd>{formatShortcutDisplay(actionShortcuts.copy)}</kbd>
                </button>
                <button type="button" role="menuitem" onClick={() => runAction(() => selectedClip.is_pinned ? void handleUnpin(selectedClip) : setPinNoteDialog({ clip: selectedClip }))}>
                  {selectedClip.is_pinned ? 'Unpin' : 'Pin'} <kbd>{formatShortcutDisplay(actionShortcuts.pin)}</kbd>
                </button>
                {selectedClip.content_type !== 'image' && <button type="button" role="menuitem" onClick={() => runAction(() => setEditDialog({ clip: selectedClip }))}>Edit <kbd>{formatShortcutDisplay(actionShortcuts.edit)}</kbd></button>}
                {selectedClip.content_type === 'image' && <button type="button" role="menuitem" onClick={() => runAction(() => void handleSaveImage(selectedClip))}>Save… <kbd>{formatShortcutDisplay(actionShortcuts.save)}</kbd></button>}
                {auth.variant === 'Authenticated' && <button type="button" role="menuitem" onClick={() => runAction(() => void sendClip(selectedClip))}>Send <kbd>{formatShortcutDisplay(actionShortcuts.send)}</kbd></button>}
                <div role="separator" />
                <button type="button" role="menuitem" className="destructive" onClick={() => runAction(() => setDeleteDialog({ clip: selectedClip }))}>Delete <kbd>{formatShortcutDisplay('CmdOrCtrl+Backspace')}</kbd></button>
              </div>
            )}
          </div>
        </footer>
      </div>

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

      {deleteDialog && (
        <ConfirmDialog
          open
          title="Delete clip?"
          body="Permanently remove this clip from your local history. This can't be undone."
          primaryLabel="Delete"
          secondaryLabel="Cancel"
          tone="destructive"
          onConfirm={() => {
            handleDelete(deleteDialog.clip.id);
            setDeleteDialog(null);
          }}
          onCancel={() => setDeleteDialog(null)}
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

// Relay transitions never replace the local clipboard workspace.
function AuthLoadingNotice({ progress }: { progress: AuthProgress }) {
  const [timedOut, setTimedOut] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setTimedOut(true), 5 * 60 * 1000);
    return () => clearTimeout(timer);
  }, []);

  const heading = timedOut
    ? 'Sign-in timed out.'
    : progress.kind === 'SigningIn'
      ? 'Signing in...'
      : progress.kind === 'Pairing'
        ? 'Pairing device...'
        : 'Rotating token...';

  return (
    <div style={S.connectionNotice} role="status">
      <span>{heading} Local history is available.</span>
      <button style={dialogStyles.btnGhost} onClick={() => {
        void signOut().catch((e) => console.error('cancel auth failed:', e));
      }}>
        {timedOut ? 'Back to local mode' : 'Stop sign-in'}
      </button>
    </div>
  );
}

function AuthErrorNotice({ reason }: { reason: AuthErrorReason }) {
  const [retrying, setRetrying] = useState(false);
  const [retryFailed, setRetryFailed] = useState(false);
  const label =
    reason.kind === 'RelayUnreachable'
      ? 'Relay unreachable'
      : reason.kind === 'KeyringUnavailable'
        ? 'Keyring unavailable'
        : reason.kind === 'NetworkDown'
          ? 'No network connection'
          : 'Invalid pair token';

  return (
    <div style={S.connectionNotice} role="status">
      <span>{retryFailed ? 'Retry failed.' : `${label}.`} Local history is available.</span>
      <button
        style={dialogStyles.btnGhost}
        disabled={retrying}
        onClick={async () => {
          setRetrying(true);
          setRetryFailed(false);
          try {
            await retryAuth();
          } catch {
            setRetryFailed(true);
          } finally {
            setRetrying(false);
          }
        }}
      >
        Retry now
      </button>
    </div>
  );
}

// ─── Internal helper components ────────────────────────────

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
        { keys: [formatShortcutDisplay(PINNED_SHORTCUT)], label: 'Toggle pinned view (unless assigned to a clip action)' },
        { keys: [formatShortcutDisplay(ACTIONS_SHORTCUT)], label: 'Open actions (unless assigned to a clip action)' },
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
        { keys: [formatShortcutDisplay(actionShortcuts.save)], label: 'Save selected image' },
        { keys: [formatShortcutDisplay(actionShortcuts.send)], label: 'Send / broadcast selected clip' },
        { keys: ['⌘⌫'], label: 'Delete selected clip' },
      ],
    },
    {
      title: 'Search',
      rows: [
        { keys: ['⌘F', '/'], label: 'Focus search' },
        { keys: ['Esc'], label: 'Return to history / clear search' },
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
  connectionNotice: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    padding: '6px 16px',
    flexShrink: 0,
    borderBottom: `1px solid ${C.border}`,
    color: C.t2,
    background: C.card2,
    fontSize: 12,
  },
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
