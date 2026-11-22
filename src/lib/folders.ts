import type { Folder, FolderRole } from './api';
import type { IconName } from './components/Icon.svelte';
import { t, type MessageKey } from './i18n';

const roleIcons: Record<FolderRole, IconName> = {
  inbox: 'inbox',
  drafts: 'draft',
  sent: 'sent',
  archive: 'archive',
  junk: 'junk',
  trash: 'trash',
  all: 'mail',
  flagged: 'flag',
};

export function folderIcon(folder: Folder): IconName {
  return folder.role ? roleIcons[folder.role] : 'folder';
}

/** Spezialordner bekommen deutsche Namen, alle anderen behalten ihren. */
export function folderLabel(folder: Folder): string {
  return folder.role ? t(`folder.${folder.role}` as MessageKey) : folder.name;
}

export interface FolderRow {
  folder: Folder;
  depth: number;
}

/**
 * Baut die Zeilen für die Seitenleiste. Manche Server legen alles unter
 * „INBOX.“ ab – dieses Präfix wird dann nicht als Ebene gezählt.
 */
export function folderRows(folders: Folder[]): FolderRow[] {
  const others = folders.filter((f) => f.path.toUpperCase() !== 'INBOX');
  const underInbox =
    others.length > 0 &&
    others.every((f) => f.delimiter && f.path.toUpperCase().startsWith(`INBOX${f.delimiter}`));

  return folders.map((folder) => {
    if (folder.role || !folder.delimiter) return { folder, depth: 0 };
    let parts = folder.path.split(folder.delimiter);
    if (underInbox) parts = parts.slice(1);
    return { folder, depth: Math.max(0, parts.length - 1) };
  });
}
