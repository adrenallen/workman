export type TodoStatus = 'backlog' | 'open' | 'in_progress' | 'completed';
export type TodoPriority = 'high' | 'medium' | 'low';

export interface TodoSummary {
  id: number;
  project_id: number;
  title: string;
  body_chars: number;
  priority: TodoPriority;
  status: TodoStatus;
  completed: boolean;
  locked_by: string | null;
  comment_count: number;
  tags: string[];
  blocker_ids: number[];
  is_blocked: boolean;
  unresolved_blocker_count: number;
}

export interface TodoView extends Omit<TodoSummary, 'body_chars'> {
  body: string;
  lock_expiry: number | null;
}

export interface TodoComment {
  id: number;
  todo_id: number;
  actor: string;
  body: string;
  created_at: number;
  updated_at: number;
}

export type TodoActivityKind = 'created' | 'completed' | 'reopened' | 'locked' | 'unlocked';

export interface TodoActivity {
  id: number;
  todo_id: number;
  actor: string;
  kind: TodoActivityKind;
  created_at: number;
}

export interface TodoDetail {
  todo: TodoView;
  comments: TodoComment[];
  comment_total_count: number;
  activity: TodoActivity[];
}

export interface ScratchpadSummary {
  id: number;
  project_id: number;
  name: string;
  revision: number;
  archived: boolean;
  tags: string[];
  matched_fields: string[];
  match_snippet?: string;
}

export interface Scratchpad {
  id: number;
  project_id: number;
  name: string;
  content: string;
  revision: number;
  tags: string[];
  archived: boolean;
}

export interface ScratchpadRead {
  scratchpad: Scratchpad;
  total_lines: number;
}

export interface CoordinationSnapshot {
  project_id: number;
  todos: TodoSummary[];
  todo_total_count: number;
  scratchpads: ScratchpadSummary[];
  scratchpad_total_count: number;
  archived_scratchpads: ScratchpadSummary[];
  archived_scratchpad_total_count: number;
}

export interface NewTodoInput {
  title: string;
  body: string;
  priority: TodoPriority;
  tags: string[];
  blocker_ids: number[];
}

export interface UpdateTodoInput {
  title?: string;
  body?: string;
  priority?: TodoPriority;
  status?: TodoStatus;
  tags?: string[];
}

export interface NewScratchpadInput {
  name: string;
  content: string;
  tags: string[];
}

export interface TodoCompleteResult {
  todo: TodoView;
  affected_todo_ids: number[];
}

export interface CoordinationClient {
  coordinationSnapshot(projectId: number): Promise<CoordinationSnapshot>;
  coordinationTodo(projectId: number, todoId: number): Promise<TodoDetail>;
  coordinationTodoCreate(projectId: number, input: NewTodoInput): Promise<TodoView>;
  coordinationTodoComplete(
    projectId: number,
    todoId: number,
    completed: boolean
  ): Promise<TodoCompleteResult>;
  coordinationTodoComment(projectId: number, todoId: number, body: string): Promise<TodoComment>;
  coordinationScratchpad(projectId: number, scratchpadId: number): Promise<ScratchpadRead>;
  coordinationScratchpadCreate(projectId: number, input: NewScratchpadInput): Promise<Scratchpad>;
  coordinationScratchpadUpdate(
    projectId: number,
    scratchpadId: number,
    expectedRevision: number,
    content: string
  ): Promise<ScratchpadRead>;
}
