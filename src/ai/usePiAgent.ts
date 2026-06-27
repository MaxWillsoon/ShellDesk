import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  Api,
  AssistantMessage,
  AssistantMessageEvent,
  AssistantMessageEventStream,
  Context,
  Message,
  Model,
  SimpleStreamOptions,
  ToolCall,
  Usage,
} from '@earendil-works/pi-ai';
import { Agent } from '@earendil-works/pi-agent-core';
import type { AgentEvent, AgentMessage, AgentTool } from '@earendil-works/pi-agent-core';
import { t, type AppLanguage } from '../i18n';
import {
  createMessageId,
  createModelsForSettings,
  getAiApiKey,
  getAiModel,
  isAiConfigured,
} from './PiAgentService';
import type { AiMessage, AiTokenUsage } from './types';

interface UsePiAgentConfig {
  settings: ShellDeskAppSettings;
  language: AppLanguage;
  systemPrompt: string;
  tools?: AgentTool[];
  connectionId?: string;
}

const AI_PROVIDER_TIMEOUT_MS = 45000;

function isAssistantMessage(message: AgentMessage): message is AssistantMessage {
  return typeof message === 'object' && message !== null && 'role' in message && message.role === 'assistant';
}

function isLlmMessage(message: AgentMessage): message is Message {
  return typeof message === 'object'
    && message !== null
    && 'role' in message
    && (message.role === 'user' || message.role === 'assistant' || message.role === 'toolResult');
}

function getMessageText(message: AgentMessage): string {
  if (!message || typeof message !== 'object' || !('content' in message)) {
    return '';
  }

  if (typeof message.content === 'string') {
    return message.content;
  }

  if (!Array.isArray(message.content)) {
    return '';
  }

  return message.content
    .filter((content) => content.type === 'text')
    .map((content) => content.text)
    .join('');
}

function hasToolCalls(message: AssistantMessage): boolean {
  return Array.isArray(message.content) && message.content.some((content) => content.type === 'toolCall');
}

function usageToTokenUsage(usage: Usage): AiTokenUsage {
  return {
    input: usage.input,
    output: usage.output,
    cost: usage.cost.total,
  };
}

function upsertMessage(messages: AiMessage[], message: AiMessage): AiMessage[] {
  return messages.some((existing) => existing.id === message.id)
    ? messages.map((existing) => (existing.id === message.id ? message : existing))
    : [...messages, message];
}

function createAgent(
  config: UsePiAgentConfig,
  onBackendMessage?: (message: AssistantMessage) => void,
): Agent | null {
  if (!isAiConfigured(config.settings)) {
    return null;
  }

  const models = createModelsForSettings(config.settings);
  const model = getAiModel(config.settings, models);
  const apiKey = getAiApiKey(config.settings);

  return new Agent({
    initialState: {
      systemPrompt: config.systemPrompt,
      model,
      tools: config.tools ?? [],
    },
    streamFn: window.guiSSH?.ai?.chat
      ? createBackendStreamFn(config.settings, onBackendMessage)
      : (streamModel, context, options) => models.streamSimple(streamModel, context, {
        ...options,
        timeoutMs: AI_PROVIDER_TIMEOUT_MS,
        maxRetries: 0,
      }),
    getApiKey: () => apiKey,
    convertToLlm: (messages) => messages.filter(isLlmMessage),
  });
}

function usageZero(): Usage {
  return {
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheWrite: 0,
    totalTokens: 0,
    cost: {
      input: 0,
      output: 0,
      cacheRead: 0,
      cacheWrite: 0,
      total: 0,
    },
  };
}

function messageContentText(message: Message): string {
  if (typeof message.content === 'string') {
    return message.content;
  }

  return message.content
    .filter((content) => content.type === 'text')
    .map((content) => content.text)
    .join('');
}

function toolCallSummary(toolCall: ToolCall): string {
  return `Tool call: ${toolCall.name}(${JSON.stringify(toolCall.arguments)})`;
}

function messageToolCalls(message: AssistantMessage): ShellDeskAiToolCall[] {
  return message.content
    .filter((content): content is ToolCall => content.type === 'toolCall')
    .map((toolCall) => ({
      id: toolCall.id,
      name: toolCall.name,
      arguments: toolCall.arguments,
    }));
}

function contextToChatMessages(systemPrompt: string | undefined, messages: Message[]): ShellDeskAiChatMessage[] {
  const chatMessages: ShellDeskAiChatMessage[] = [];

  if (systemPrompt?.trim()) {
    chatMessages.push({
      role: 'system',
      content: systemPrompt,
    });
  }

  for (const message of messages.slice(-16)) {
    if (message.role === 'user') {
      chatMessages.push({
        role: 'user',
        content: messageContentText(message),
      });
      continue;
    }

    if (message.role === 'assistant') {
      const text = messageContentText(message);
      const toolCalls = messageToolCalls(message);
      const fallbackContent = [text, ...toolCalls.map((toolCall) => toolCallSummary({
        type: 'toolCall',
        id: toolCall.id,
        name: toolCall.name,
        arguments: toolCall.arguments,
      }))].filter(Boolean).join('\n\n');

      if (text.trim() || toolCalls.length > 0) {
        chatMessages.push({
          role: 'assistant',
          content: text || fallbackContent,
          toolCalls,
        });
      }
      continue;
    }

    if (message.role === 'toolResult') {
      chatMessages.push({
        role: 'tool',
        content: messageContentText(message),
        toolCallId: message.toolCallId,
        toolName: message.toolName,
      });
    }
  }

  return chatMessages.filter((message) => message.content.trim());
}

function createAssistantMessage(
  model: Model<Api>,
  content: string,
  toolCalls: ShellDeskAiToolCall[],
  stopReason: AssistantMessage['stopReason'] = toolCalls.length ? 'toolUse' : 'stop',
): AssistantMessage {
  return {
    role: 'assistant',
    api: model.api,
    provider: model.provider,
    model: model.id,
    content: [
      ...(content.trim() ? [{ type: 'text' as const, text: content }] : []),
      ...toolCalls.map((toolCall) => ({
        type: 'toolCall' as const,
        id: toolCall.id,
        name: toolCall.name,
        arguments: toolCall.arguments,
      })),
    ],
    usage: usageZero(),
    stopReason,
    timestamp: Date.now(),
  };
}

function createErrorAssistantMessage(model: Model<Api>, error: unknown): AssistantMessage {
  return {
    ...createAssistantMessage(model, '', [], 'error'),
    errorMessage: error instanceof Error ? error.message : String(error),
  };
}

async function requestBackendAssistantMessage(
  settings: ShellDeskAppSettings,
  model: Model<Api>,
  context: Context,
  options?: SimpleStreamOptions,
): Promise<AssistantMessage> {
  const backendChat = window.guiSSH?.ai?.chat;

  if (!backendChat) {
    return createErrorAssistantMessage(model, new Error('AI backend is unavailable'));
  }

  try {
    if (options?.signal?.aborted) {
      throw new Error('Request was aborted');
    }

    const timeout = new Promise<never>((_, reject) => {
      window.setTimeout(() => reject(new Error('AI request timed out.')), AI_PROVIDER_TIMEOUT_MS);
    });
    const result = await Promise.race([backendChat({
      provider: settings.aiProvider,
      apiFormat: settings.aiApiFormat,
      apiBaseUrl: settings.aiApiBaseUrl,
      apiKey: settings.aiApiKey,
      model: settings.aiModel || model.id,
      messages: contextToChatMessages(context.systemPrompt, context.messages),
      tools: context.tools?.map((tool) => ({
        name: tool.name,
        description: tool.description,
        parameters: tool.parameters,
      })),
      temperature: options?.temperature ?? 0.2,
    }), timeout]);

    if (options?.signal?.aborted) {
      throw new Error('Request was aborted');
    }

    return createAssistantMessage(model, result.content ?? '', result.toolCalls ?? []);
  } catch (error) {
    return createErrorAssistantMessage(model, error);
  }
}

function createAssistantEvents(message: AssistantMessage): AssistantMessageEvent[] {
  if (message.stopReason === 'error' || message.stopReason === 'aborted') {
    return [{ type: 'error', reason: message.stopReason, error: message }];
  }

  return [{
    type: 'done',
    reason: message.stopReason === 'toolUse' ? 'toolUse' : 'stop',
    message,
  }];
}

function createBackendStreamFn(
  settings: ShellDeskAppSettings,
  onBackendMessage?: (message: AssistantMessage) => void,
) {
  return (model: Model<Api>, context: Context, options?: SimpleStreamOptions) => {
    const finalMessagePromise = requestBackendAssistantMessage(settings, model, context, options)
      .then((message) => {
        onBackendMessage?.(message);
        return message;
      });
    return {
      async *[Symbol.asyncIterator]() {
        const message = await finalMessagePromise;
        for (const event of createAssistantEvents(message)) {
          yield event;
        }
      },
      result: () => finalMessagePromise,
    } as unknown as AssistantMessageEventStream;
  };
}

export function usePiAgent(config: UsePiAgentConfig) {
  const [messages, setMessages] = useState<AiMessage[]>([]);
  const [isBusy, setIsBusy] = useState(false);
  const [busyText, setBusyText] = useState('');
  const [error, setError] = useState('');
  const [tokenUsage, setTokenUsage] = useState<AiTokenUsage | null>(null);
  const agentRef = useRef<Agent | null>(null);
  const unsubscribeRef = useRef<(() => void) | null>(null);
  const assistantMessageIdsRef = useRef(new WeakMap<AssistantMessage, string>());
  const isMountedRef = useRef(true);
  const isConfigured = isAiConfigured(config.settings);

  const agentKey = useMemo(() => JSON.stringify({
    provider: config.settings.aiProvider,
    apiFormat: config.settings.aiApiFormat,
    apiBaseUrl: config.settings.aiApiBaseUrl,
    apiKey: config.settings.aiApiKey,
    model: config.settings.aiModel,
    systemPrompt: config.systemPrompt,
    connectionId: config.connectionId,
    toolNames: (config.tools ?? []).map((tool) => tool.name),
  }), [config.connectionId, config.settings, config.systemPrompt, config.tools]);

  const disposeAgent = useCallback(() => {
    unsubscribeRef.current?.();
    unsubscribeRef.current = null;
    agentRef.current?.abort();
    agentRef.current = null;
    assistantMessageIdsRef.current = new WeakMap();
  }, []);

  const commitAssistantMessage = useCallback((message: AssistantMessage) => {
    const id = assistantMessageIdsRef.current.get(message) ?? createMessageId();
    assistantMessageIdsRef.current.set(message, id);
    const content = getMessageText(message);

    if (content.trim()) {
      setMessages((prev) => upsertMessage(prev, {
        id,
        role: 'assistant',
        content,
        createdAt: new Date(message.timestamp || Date.now()).toISOString(),
      }));
    }

    if (message.usage) {
      setTokenUsage(usageToTokenUsage(message.usage));
    }

    if (message.stopReason === 'error' && message.errorMessage) {
      setError(message.errorMessage);
      setIsBusy(false);
      setBusyText('');
      return;
    }

    if (!hasToolCalls(message)) {
      setIsBusy(false);
      setBusyText('');
    }
  }, []);

  const ensureAgent = useCallback(() => {
    if (agentRef.current) {
      return agentRef.current;
    }

    const agent = createAgent(config, commitAssistantMessage);

    if (!agent) {
      return null;
    }

    unsubscribeRef.current = agent.subscribe((event: AgentEvent) => {
      if (!isMountedRef.current) {
        return;
      }

      if (event.type === 'agent_start') {
        setIsBusy(true);
        setBusyText(t('auto.aiChat.thinking', config.language));
        setError('');
        return;
      }

      if (event.type === 'message_start' && isAssistantMessage(event.message)) {
        if (!assistantMessageIdsRef.current.has(event.message)) {
          assistantMessageIdsRef.current.set(event.message, createMessageId());
        }
        return;
      }

      if (event.type === 'message_update' && isAssistantMessage(event.message)) {
        commitAssistantMessage(event.message);
        return;
      }

      if (event.type === 'message_end' && isAssistantMessage(event.message)) {
        commitAssistantMessage(event.message);
        return;
      }

      if (event.type === 'tool_execution_start') {
        setIsBusy(true);
        setBusyText(t('auto.aiChat.toolRunning', config.language, { value0: event.toolName }));
        return;
      }

      if (event.type === 'tool_execution_end' && event.isError) {
        setError(t('auto.aiChat.toolFailed', config.language, { value0: event.toolName }));
        setBusyText('');
        setIsBusy(false);
        return;
      }

      if (event.type === 'tool_execution_end') {
        setBusyText(t('auto.aiChat.thinking', config.language));
        return;
      }

      if (event.type === 'agent_end') {
        setIsBusy(false);
        setBusyText('');
      }
    });

    agentRef.current = agent;
    return agent;
  }, [commitAssistantMessage, config]);

  useEffect(() => {
    disposeAgent();
    setMessages([]);
    setError('');
    setIsBusy(false);
    setBusyText('');
    setTokenUsage(null);

    return disposeAgent;
  }, [agentKey, disposeAgent]);

  useEffect(() => () => {
    isMountedRef.current = false;
    disposeAgent();
  }, [disposeAgent]);

  const sendMessage = useCallback(async (content: string) => {
    const trimmedContent = content.trim();

    if (!trimmedContent || isBusy) {
      return;
    }

    if (!isConfigured) {
      setError(t('auto.aiChat.notConfiguredError', config.language));
      return;
    }

    const agent = ensureAgent();

    if (!agent) {
      setError(t('auto.aiChat.notConfiguredError', config.language));
      return;
    }

    const userMessage: AiMessage = {
      id: createMessageId(),
      role: 'user',
      content: trimmedContent,
      createdAt: new Date().toISOString(),
    };

    setMessages((prev) => [...prev, userMessage]);
    setError('');
    setIsBusy(true);
    setBusyText(t('auto.aiChat.thinking', config.language));

    try {
      await agent.prompt(trimmedContent);
    } catch (err) {
      if (isMountedRef.current) {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      if (isMountedRef.current) {
        setIsBusy(false);
        setBusyText('');
      }
    }
  }, [config.language, ensureAgent, isBusy, isConfigured]);

  const cancelRequest = useCallback(() => {
    agentRef.current?.abort();
    setIsBusy(false);
    setBusyText('');
  }, []);

  const clearHistory = useCallback(() => {
    disposeAgent();
    setMessages([]);
    setError('');
    setIsBusy(false);
    setBusyText('');
    setTokenUsage(null);
  }, [disposeAgent]);

  return {
    messages,
    isBusy,
    busyText,
    error,
    isConfigured,
    sendMessage,
    cancelRequest,
    clearHistory,
    tokenUsage,
  };
}
