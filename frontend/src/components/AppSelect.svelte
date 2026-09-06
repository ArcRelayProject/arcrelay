<script lang="ts" generics="T extends SelectValue | SelectValue[]">
  import { Select } from "bits-ui";
  import { CaretDown, Check } from "phosphor-svelte";
  import { selectKey, selectedOption, type SelectOption, type SelectValue } from "../selectOptions";

  type Props = Pick<Select.TriggerProps, "id" | "class" | "style" | "title" | "aria-label" | "aria-labelledby" | "aria-describedby"> & {
    value?: T;
    options: readonly SelectOption[];
    multiple?: boolean;
    disabled?: boolean;
    placeholder?: string;
    onValueChange?: (value: T) => void;
  };

  let {
    value = $bindable(),
    options,
    multiple = false,
    placeholder = "—",
    disabled = false,
    class: className = "",
    onValueChange,
    ...triggerProps
  }: Props = $props();
  let trigger = $state<HTMLButtonElement | null>(null);
  let open = $state(false);
  // A native modal dialog lives in the browser's top layer. Portalling to body
  // would put the menu behind that dialog and make its options inert.
  const portalTarget = $derived(trigger?.closest("dialog") ?? undefined);
  const items = $derived(options.map((option) => ({ ...option, value: selectKey(option.value) })));
  const singleValue = $derived(value !== undefined && !Array.isArray(value) ? selectKey(value) : "");
  const multipleValues = $derived(Array.isArray(value) ? value.map(selectKey) : []);
  const selected = $derived.by(() => {
    const current = value;
    return Array.isArray(current)
      ? options.filter((option) => current.includes(option.value))
      : options.filter((option) => option.value === current).slice(0, 1);
  });
  const label = $derived(selected.map((option) => option.label).join(", ") || placeholder);

  $effect(() => {
    if (disabled) open = false;
  });

  function changeSingle(key: string) {
    const option = selectedOption(options, key);
    if (!option || option.disabled || disabled) return;
    value = option.value as T;
    onValueChange?.(value);
  }

  function changeMultiple(keys: string[]) {
    if (disabled) return;
    value = keys.flatMap((key) => {
      const option = selectedOption(options, key);
      return option ? [option.value] : [];
    }) as T;
    onValueChange?.(value);
  }
</script>

{#snippet control()}
  <Select.Trigger
    {...triggerProps}
    bind:ref={trigger}
    class={`app-select-trigger ${className}`}
    title={triggerProps.title ?? label}
  >
    <span class="app-select-label" lang={selected.length === 1 ? selected[0].lang : undefined}>{label}</span>
    <CaretDown class="app-select-chevron" size={15} aria-hidden="true" />
  </Select.Trigger>
  <Select.Portal to={portalTarget}>
    <Select.Content class="app-select-content" sideOffset={5} align="start" collisionPadding={8} strategy="fixed"
      aria-label={triggerProps["aria-label"]}
      aria-labelledby={triggerProps["aria-label"] ? undefined : (triggerProps["aria-labelledby"] ?? trigger?.id)}
    >
      <Select.Viewport class="app-select-viewport">
        {#each items as option}
          <Select.Item class="app-select-item" value={option.value} label={option.label} disabled={option.disabled} lang={option.lang}>
            {#snippet children({ selected })}
              <span>{option.label}</span>
              <span class="app-select-check">{#if selected}<Check size={15} weight="bold" aria-hidden="true" />{/if}</span>
            {/snippet}
          </Select.Item>
        {/each}
      </Select.Viewport>
    </Select.Content>
  </Select.Portal>
{/snippet}

{#if multiple}
  <Select.Root type="multiple" value={multipleValues} onValueChange={changeMultiple} bind:open {disabled} {items}>
    {@render control()}
  </Select.Root>
{:else}
  <Select.Root type="single" value={singleValue} onValueChange={changeSingle} bind:open {disabled} {items} allowDeselect={false}>
    {@render control()}
  </Select.Root>
{/if}

<style>
  :global(.app-select-trigger) {
    -webkit-appearance: none;
    appearance: none;
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-width: 0;
    min-height: 38px;
    padding: 8px 11px;
    border: 1px solid var(--border-strong, #dcdde7);
    border-radius: 8px;
    background: var(--control-bg, #fff);
    background-image: none;
    color: var(--text, #252630);
    box-shadow: none;
    font: inherit;
    font-size: 13px;
    line-height: 1.4;
    text-align: left;
    cursor: pointer;
  }
  :global(.app-select-trigger:hover:not(:disabled)) { border-color: var(--accent, #5b5ff0); }
  :global(.app-select-trigger:focus-visible), :global(.app-select-trigger[data-state="open"]) {
    outline: 2px solid var(--accent, #5b5ff0);
    outline-offset: 2px;
  }
  :global(.app-select-trigger:disabled) { opacity: .5; cursor: not-allowed; }
  .app-select-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  :global(.app-select-chevron) { flex-shrink: 0; color: var(--text-muted, #767888); }
  :global(.app-select-content) {
    box-sizing: border-box;
    z-index: 1000;
    min-width: var(--bits-select-anchor-width);
    max-width: calc(100vw - 16px);
    max-height: min(320px, var(--bits-select-content-available-height));
    overflow: hidden;
    padding: 4px;
    border: 1px solid var(--border-strong, #dcdde7);
    border-radius: 9px;
    background: var(--surface-raised, #fff);
    color: var(--text, #252630);
    box-shadow: 0 10px 30px rgba(0, 0, 0, .18);
    font-family: inherit;
    font-size: 13px;
    outline: none;
  }
  :global(.app-select-viewport) {
    max-height: min(310px, calc(var(--bits-select-content-available-height) - 10px));
    overflow: auto;
    overscroll-behavior: contain;
  }
  :global(.app-select-item) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 34px;
    padding: 6px 9px;
    border-radius: 5px;
    line-height: 1.4;
    overflow-wrap: anywhere;
    cursor: default;
    outline: none;
    user-select: none;
  }
  :global(.app-select-item[data-highlighted]) { background: var(--accent-soft, #efeeff); color: var(--accent-strong, #514ee8); }
  :global(.app-select-item[data-selected]) { color: var(--accent-strong, #514ee8); }
  :global(.app-select-item[data-disabled]) { opacity: .45; }
  .app-select-check { display: inline-flex; width: 15px; flex-shrink: 0; }
</style>
