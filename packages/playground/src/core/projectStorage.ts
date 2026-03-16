import type { SerializedState } from "./urlState";

export interface StoredProject {
  name: string;
  state: SerializedState;
  updatedAt: number;
}

const STORAGE_KEY = "verter-playground-projects";

function readAll(): StoredProject[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    return JSON.parse(raw) as StoredProject[];
  } catch {
    return [];
  }
}

function writeAll(projects: StoredProject[]): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(projects));
}

export function listProjects(): StoredProject[] {
  return readAll().sort((a, b) => b.updatedAt - a.updatedAt);
}

export function getProject(name: string): StoredProject | null {
  return readAll().find((p) => p.name === name) ?? null;
}

export function saveProject(name: string, state: SerializedState): void {
  const projects = readAll();
  const idx = projects.findIndex((p) => p.name === name);
  const entry: StoredProject = { name, state, updatedAt: Date.now() };
  if (idx >= 0) {
    projects[idx] = entry;
  } else {
    projects.push(entry);
  }
  writeAll(projects);
}

export function deleteProject(name: string): void {
  const projects = readAll().filter((p) => p.name !== name);
  writeAll(projects);
}

export function renameProject(oldName: string, newName: string): boolean {
  const projects = readAll();
  const idx = projects.findIndex((p) => p.name === oldName);
  if (idx < 0) return false;
  if (projects.some((p) => p.name === newName)) return false;
  projects[idx].name = newName;
  projects[idx].updatedAt = Date.now();
  writeAll(projects);
  return true;
}
