import { commands, type VersionStatus } from '../bindings';
import { C } from '../design';

interface Props {
    status: VersionStatus;
    isOwnDesktop: boolean;
    clientType: string | null;
}

const BTN_STYLE: React.CSSProperties = {
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
};

const LINK_STYLE: React.CSSProperties = {
    color: C.t1,
    fontSize: 12,
    textDecoration: 'underline',
    fontFamily: 'var(--font-body)',
    whiteSpace: 'nowrap',
};

export function DeviceUpdateAction({ status, isOwnDesktop, clientType }: Props) {
    if (status !== 'Outdated') return null;

    if (isOwnDesktop) {
        return (
            <button
                type="button"
                style={BTN_STYLE}
                onClick={() => {
                    void commands.runSelfUpdate();
                }}
            >
                Update
            </button>
        );
    }

    const anchor = clientType === 'desktop' ? 'desktop' : 'cli';
    return (
        <a
            href={`https://cinchcli.com/docs/update#${anchor}`}
            target="_blank"
            rel="noopener noreferrer"
            style={LINK_STYLE}
        >
            How to update
        </a>
    );
}
