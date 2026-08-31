export type ProjectMenuPullRequestState = 'draft' | 'open' | 'merged' | 'closed';

export interface ProjectMenuPullRequest {
  number: number;
  state: ProjectMenuPullRequestState;
}

export interface ProjectFrequentAction {
  id: 'open-pull-request' | 'open-in-editor' | 'open-in-finder' | 'open-herd-site';
  label: string;
  detail?: string;
  pullRequestState?: ProjectMenuPullRequestState;
}

export function projectFrequentActions(input: {
  editorLabel: string;
  pullRequest?: ProjectMenuPullRequest | null;
  siteUrl?: string | null;
}): ProjectFrequentAction[] {
  const items: ProjectFrequentAction[] = [];
  const pullRequest = input.pullRequest;

  if (pullRequest) {
    items.push({
      id: 'open-pull-request',
      label: `Open PR #${pullRequest.number} on GitHub`,
      detail: `${pullRequest.state[0].toUpperCase()}${pullRequest.state.slice(1)} pull request`,
      pullRequestState: pullRequest.state
    });
  }

  items.push(
    { id: 'open-in-editor', label: input.editorLabel },
    { id: 'open-in-finder', label: 'Show in file browser' }
  );

  if (input.siteUrl) {
    items.push({ id: 'open-herd-site', label: 'Open app', detail: input.siteUrl });
  }

  return items;
}
