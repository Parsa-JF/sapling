/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export function notEmpty<T>(value: T | null | undefined): value is T {
  return value !== null && value !== undefined;
}

/**
 * Throw if value is `null` or `undefined`.
 */
export function unwrap<T>(value: T | undefined | null): T {
  if (value == null) {
    throw new Error(`expected value not to be ${value}`);
  }
  return value;
}

/**
 * generate a small random ID string via time in ms + random number encoded as a [0-9a-z]+ string
 * This should not be used for cryptographic purposes or if universal uniqueness is absolutely necessary
 */
export function randomId(): string {
  return Date.now().toString(36) + Math.random().toString(36);
}

export type Deferred<T> = {
  promise: Promise<T>;
  resolve: (t: T) => void;
  reject: (e: Error) => void;
};
/**
 * Wraps `new Promise<T>()`, so you can access resolve/reject outside of the callback.
 * Useful for externally resolving promises in tests.
 */
export function defer<T>(): Deferred<T> {
  const deferred = {
    promise: undefined as unknown as Promise<T>,
    resolve: undefined as unknown as (t: T) => void,
    reject: undefined as unknown as (e: Error) => void,
  };
  deferred.promise = new Promise<T>((resolve: (t: T) => void, reject: (e: Error) => void) => {
    deferred.resolve = resolve;
    deferred.reject = reject;
  });
  return deferred;
}

/**
 * Returns the part of the string after the last occurrence of delimiter,
 * or the entire string if no matches are found.
 * (default delimiter is '/')
 *
 * ```
 * basename('/path/to/foo.txt', '/') -> 'foo.txt'
 * basename('foo.txt', '/') -> 'foo.txt'
 * basename('/path/', '/') -> ''
 * ```
 */
export function basename(s: string, delimiter = '/') {
  const foundIndex = s.lastIndexOf(delimiter);
  if (foundIndex === -1) {
    return s;
  }
  return s.slice(foundIndex + 1);
}

export function findParentWithClassName(
  start: HTMLElement,
  className: string,
): HTMLElement | undefined {
  let el = start as HTMLElement | null;
  while (el) {
    if (el.classList?.contains(className)) {
      return el;
    } else {
      el = el.parentElement;
    }
  }
  return undefined;
}

/**
 * Applies a function to each key & value in an Object.
 * ```
 * mapObject(
 *   {foo: 1, bar: 2},
 *   ([key, value]) => ['_' + key, value + 1]
 * )
 * => {_foo: 2, _bar: 3}
 * ```
 */
export function mapObject<K1 extends string | number, V1, K2 extends string | number, V2>(
  o: Record<K1, V1>,
  func: (param: [K1, V1]) => [K2, V2],
): Record<K2, V2> {
  return Object.fromEntries((Object.entries(o) as Array<[K1, V1]>).map(func)) as Record<K2, V2>;
}

/**
 * Test if a generator yields the given value.
 */
export function generatorContains<V>(gen: Generator<V>, value: V): boolean {
  for (const v of gen) {
    if (v === value) {
      return true;
    }
  }
  return false;
}

/**
 * Zip 2 iterators.
 */
export function* zip<T, U>(iter1: Iterable<T>, iter2: Iterable<U>): IterableIterator<[T, U]> {
  const iterator1 = iter1[Symbol.iterator]();
  const iterator2 = iter2[Symbol.iterator]();
  while (true) {
    const result1 = iterator1.next();
    const result2 = iterator2.next();
    if (result1.done || result2.done) {
      break;
    }
    yield [result1.value, result2.value];
  }
}

/** Truncate a long string. */
export function truncate(text: string, maxLength = 100): string {
  return text.length > maxLength ? text.substring(0, Math.max(0, maxLength - 1)) + '…' : text;
}

export function isPromise<T>(o: unknown): o is Promise<T> {
  return typeof (o as {then?: () => void})?.then === 'function';
}
