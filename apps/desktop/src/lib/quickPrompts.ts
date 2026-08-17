export interface QuickPrompt {
  id: number;
  name: string;
  body: string;
  sort_order: number;
  created_at: number;
  updated_at: number;
}

export interface QuickPromptInput {
  id?: number;
  name: string;
  body: string;
}

export interface DeleteQuickPromptResult {
  quick_prompt_id: number;
  deleted: boolean;
}

export interface QuickPromptsClient {
  listQuickPrompts(): Promise<QuickPrompt[]>;
  saveQuickPrompt(prompt: QuickPromptInput): Promise<QuickPrompt>;
  deleteQuickPrompt(quickPromptId: number): Promise<DeleteQuickPromptResult>;
  reorderQuickPrompts(quickPromptIds: number[]): Promise<QuickPrompt[]>;
}

export interface QuickPromptsSnapshot {
  prompts: QuickPrompt[];
  loading: boolean;
  error: string | null;
}

type Subscriber = (snapshot: QuickPromptsSnapshot) => void;

/** One daemon-backed quick-prompt cache shared by the palette and Settings. */
export class QuickPromptsStore {
  private snapshot: QuickPromptsSnapshot = { prompts: [], loading: false, error: null };
  private subscribers = new Set<Subscriber>();
  private refreshPromise: Promise<QuickPrompt[]> | null = null;
  private readonly client: QuickPromptsClient;

  constructor(client: QuickPromptsClient) {
    this.client = client;
  }

  current(): QuickPromptsSnapshot {
    return this.snapshot;
  }

  subscribe(subscriber: Subscriber): () => void {
    this.subscribers.add(subscriber);
    subscriber(this.snapshot);
    return () => this.subscribers.delete(subscriber);
  }

  async refresh(force = false): Promise<QuickPrompt[]> {
    if (this.refreshPromise && !force) return this.refreshPromise;
    this.publish({ ...this.snapshot, loading: true, error: null });
    const request = this.client
      .listQuickPrompts()
      .then((prompts) => {
        this.publish({ prompts, loading: false, error: null });
        return prompts;
      })
      .catch((cause) => {
        const error = cause instanceof Error ? cause.message : String(cause);
        this.publish({ ...this.snapshot, loading: false, error });
        throw cause;
      })
      .finally(() => {
        if (this.refreshPromise === request) this.refreshPromise = null;
      });
    this.refreshPromise = request;
    return request;
  }

  async save(input: QuickPromptInput): Promise<QuickPrompt> {
    const saved = await this.client.saveQuickPrompt(input);
    const index = this.snapshot.prompts.findIndex((prompt) => prompt.id === saved.id);
    const prompts = [...this.snapshot.prompts];
    if (index >= 0) prompts[index] = saved;
    else prompts.push(saved);
    this.publish({ prompts, loading: false, error: null });
    return saved;
  }

  async remove(quickPromptId: number): Promise<DeleteQuickPromptResult> {
    const result = await this.client.deleteQuickPrompt(quickPromptId);
    if (result.deleted) {
      this.publish({
        prompts: this.snapshot.prompts.filter((prompt) => prompt.id !== quickPromptId),
        loading: false,
        error: null
      });
    }
    return result;
  }

  async reorder(quickPromptIds: number[]): Promise<QuickPrompt[]> {
    const prompts = await this.client.reorderQuickPrompts(quickPromptIds);
    this.publish({ prompts, loading: false, error: null });
    return prompts;
  }

  private publish(snapshot: QuickPromptsSnapshot): void {
    this.snapshot = snapshot;
    for (const subscriber of this.subscribers) subscriber(snapshot);
  }
}

const stores = new WeakMap<object, QuickPromptsStore>();

export function getQuickPromptsStore(client: QuickPromptsClient): QuickPromptsStore {
  const key = client as object;
  let store = stores.get(key);
  if (!store) {
    store = new QuickPromptsStore(client);
    stores.set(key, store);
  }
  return store;
}
