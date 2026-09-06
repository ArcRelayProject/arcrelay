export type SelectValue = string | number | boolean;

export interface SelectOption {
  value: SelectValue;
  label: string;
  disabled?: boolean;
  lang?: string;
}

// Bits UI uses strings internally. Keep empty strings, false, zero and numeric
// values distinct without changing the application's persisted value types.
export function selectKey(value: SelectValue): string {
  return `${typeof value}:${String(value)}`;
}

export function selectedOption(options: readonly SelectOption[], key: string) {
  return options.find((option) => selectKey(option.value) === key);
}
