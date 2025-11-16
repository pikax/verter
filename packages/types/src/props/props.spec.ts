import { describe, it, assertType } from 'vitest';
import { MakePublicProps, MakeInternalProps } from './props';
import { defineProps, withDefaults } from 'vue';

describe('Props helpers', () => {
  describe('MakePublicProps', () => {
    it('returns plain object props unchanged', () => {
      type Plain = { a: number; b?: string };
      type Public = MakePublicProps<Plain>;
      assertType<Public>({} as Plain);
      // optional stays optional
      const v1: Public = { a: 1 };
      const v2: Public = { a: 1, b: 'x' };
      void v1; void v2;
    });

    it('makes non-default props potentially undefined and keeps default props required', () => {
      const p = defineProps({
        f: String,
        d: { type: String, default: 'bar' },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;
      // f can be string | undefined
      assertType<Public['f']>({} as string | undefined);
      // d is always string
      assertType<Public['d']>({} as string);
      // @ts-expect-error d cannot be undefined
      assertType<Public['d']>({} as undefined);
      // @ts-expect-error f is readonly
      p.f = '';
    });

    it('works with withDefaults applied', () => {
      const d = withDefaults(
        defineProps<{
          f?: string;
          d?: string;
        }>(),
        { d: 'baz' }
      );
      type D = typeof d;
      type Public = MakePublicProps<D>;
      // f can be string | undefined
      assertType<Public['f']>({} as string | undefined);
      // d is required (string only)
      assertType<Public['d']>({} as string);
      // @ts-expect-error d not undefined
      assertType<Public['d']>({} as undefined);
    });

    it('handles multiple props with mixed defaults', () => {
      const p = defineProps({
        required1: String,
        required2: Number,
        withDefault1: { type: String, default: 'foo' },
        withDefault2: { type: Boolean, default: true },
        withDefault3: { type: Array as () => string[], default: () => [] },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      // Props without defaults can be undefined
      assertType<Public['required1']>({} as string | undefined);
      assertType<Public['required2']>({} as number | undefined);

      // Props with defaults are always defined
      assertType<Public['withDefault1']>({} as string);
      assertType<Public['withDefault2']>({} as boolean);
      assertType<Public['withDefault3']>({} as string[]);

      // @ts-expect-error default props cannot be undefined
      assertType<Public['withDefault1']>({} as undefined);
      // @ts-expect-error default props cannot be undefined
      assertType<Public['withDefault2']>({} as undefined);
    });

    it('handles complex prop types with defaults', () => {
      const p = defineProps({
        obj: { type: Object as () => { x: number; y: number }, default: () => ({ x: 0, y: 0 }) },
        union: { type: [String, Number] as unknown as () => string | number, default: 'default' },
        nullable: { type: [String, null] as unknown as () => string | null, default: null },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      assertType<Public['obj']>({} as { x: number; y: number });
      assertType<Public['union']>({} as string | number);
      assertType<Public['nullable']>({} as string | null);

      // @ts-expect-error cannot be undefined
      assertType<Public['obj']>({} as undefined);
    });

    it('handles all props with defaults', () => {
      const p = defineProps({
        a: { type: String, default: 'a' },
        b: { type: Number, default: 42 },
        c: { type: Boolean, default: false },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      // All props are defined (no undefined union)
      assertType<Public['a']>({} as string);
      assertType<Public['b']>({} as number);
      assertType<Public['c']>({} as boolean);

      // @ts-expect-error cannot be undefined
      assertType<Public['a']>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Public['b']>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Public['c']>({} as undefined);
    });

    it('handles all props without defaults', () => {
      const p = defineProps({
        a: String,
        b: Number,
        c: Boolean,
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      // All props can be undefined
      assertType<Public['a']>({} as string | undefined);
      assertType<Public['b']>({} as number | undefined);
      // Boolean props may have special handling - verify the actual type
      type CType = Public['c'];
      const cTest: CType = true;
      void cTest;
    });

    it('preserves readonly nature of props', () => {
      const p = defineProps({
        name: String,
        age: { type: Number, default: 0 },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      const pub = {} as Public;
      // @ts-expect-error props are readonly
      pub.name = 'test';
      // @ts-expect-error props are readonly
      pub.age = 42;
    });

    it('handles withDefaults with all props optional', () => {
      const d = withDefaults(
        defineProps<{
          a?: string;
          b?: number;
          c?: boolean;
        }>(),
        {
          a: 'default',
          b: 0,
          c: false,
        }
      );
      type D = typeof d;
      type Public = MakePublicProps<D>;

      // All have defaults, so none are undefined
      assertType<Public['a']>({} as string);
      assertType<Public['b']>({} as number);
      assertType<Public['c']>({} as boolean);

      // @ts-expect-error cannot be undefined
      assertType<Public['a']>({} as undefined);
    });

    it('handles withDefaults with partial defaults', () => {
      const d = withDefaults(
        defineProps<{
          a?: string;
          b?: number;
          c?: boolean;
        }>(),
        {
          a: 'default',
        }
      );
      type D = typeof d;
      type Public = MakePublicProps<D>;

      // a has default, not undefined
      assertType<Public['a']>({} as string);
      // b and c can be undefined
      assertType<Public['b']>({} as number | undefined);
      // Boolean props may have special handling
      type CType = Public['c'];
      const cTest: CType = false;
      void cTest;

      // @ts-expect-error a cannot be undefined
      assertType<Public['a']>({} as undefined);
    });

    it('handles withDefaults with required props', () => {
      const d = withDefaults(
        defineProps<{
          required: string;
          optional?: number;
        }>(),
        {
          optional: 42,
        }
      );
      type D = typeof d;
      type Public = MakePublicProps<D>;

      assertType<Public['required']>({} as string);
      assertType<Public['optional']>({} as number);

      // @ts-expect-error required cannot be undefined
      assertType<Public['required']>({} as undefined);
      // @ts-expect-error optional has default
      assertType<Public['optional']>({} as undefined);
    });

    it('handles empty props', () => {
      const p = defineProps({});
      type P = typeof p;
      type Public = MakePublicProps<P>;

      assertType<Public>({} as P);
    });

    it('handles single prop with default', () => {
      const p = defineProps({
        single: { type: String, default: 'value' },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      assertType<Public['single']>({} as string);
      // @ts-expect-error cannot be undefined
      assertType<Public['single']>({} as undefined);
    });

    it('handles single prop without default', () => {
      const p = defineProps({
        single: String,
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      assertType<Public['single']>({} as string | undefined);
    });
  });

  describe('MakeInternalProps', () => {
    it('makes all props required for internal usage', () => {
      const d = withDefaults(
        defineProps<{
          f?: string;
          d?: string;
        }>(),
        { d: 'baz' }
      );
      type D = typeof d;
      type Internal = MakeInternalProps<D>;
      // Both props required internally
      // @ts-expect-error f cannot be omitted internally
      assertType<Internal>({} as { d: string });
      assertType<Internal>({} as { f: string; d: string });
    });

    it('treats default + optional mix as required internally', () => {
      const p = defineProps({
        f: String,
        d: { type: String, default: 'bar' },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;
      // @ts-expect-error f must be string internally
      assertType<Internal['f']>({} as undefined);
      assertType<Internal['f']>({} as string);
      assertType<Internal['d']>({} as string);
    });

    it('makes all props required even without defaults', () => {
      const p = defineProps({
        a: String,
        b: Number,
        c: Boolean,
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      // All must be defined internally
      assertType<Internal['a']>({} as string);
      assertType<Internal['b']>({} as number);
      assertType<Internal['c']>({} as boolean);

      // @ts-expect-error cannot be undefined
      assertType<Internal['a']>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Internal['b']>({} as undefined);
    });

    it('makes all props required with all defaults', () => {
      const p = defineProps({
        a: { type: String, default: 'a' },
        b: { type: Number, default: 42 },
        c: { type: Boolean, default: false },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal['a']>({} as string);
      assertType<Internal['b']>({} as number);
      assertType<Internal['c']>({} as boolean);

      // @ts-expect-error cannot be undefined
      assertType<Internal['a']>({} as undefined);
    });

    it('makes optional props required with withDefaults', () => {
      const d = withDefaults(
        defineProps<{
          a?: string;
          b?: number;
          c?: boolean;
        }>(),
        {
          b: 0,
        }
      );
      type D = typeof d;
      type Internal = MakeInternalProps<D>;

      // All required internally
      assertType<Internal['a']>({} as string);
      assertType<Internal['b']>({} as number);
      assertType<Internal['c']>({} as boolean);

      // @ts-expect-error cannot be undefined
      assertType<Internal['a']>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Internal['c']>({} as undefined);
    });

    it('handles complex types as required', () => {
      const p = defineProps({
        obj: Object as () => { x: number; y: number },
        arr: Array as () => string[],
        union: [String, Number] as unknown as () => string | number,
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal['obj']>({} as { x: number; y: number });
      assertType<Internal['arr']>({} as string[]);
      assertType<Internal['union']>({} as string | number);

      // @ts-expect-error cannot be undefined
      assertType<Internal['obj']>({} as undefined);
      // @ts-expect-error cannot be undefined
      assertType<Internal['arr']>({} as undefined);
    });

    it('preserves readonly nature of props', () => {
      const p = defineProps({
        name: String,
        age: { type: Number, default: 0 },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      const internal = {} as Internal;
      // @ts-expect-error props are readonly
      internal.name = 'test';
      // @ts-expect-error props are readonly
      internal.age = 42;
    });

    it('handles empty props', () => {
      const p = defineProps({});
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal>({} as P);
    });

    it('makes single prop required', () => {
      const p = defineProps({
        single: String,
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal['single']>({} as string);
      // @ts-expect-error cannot be undefined
      assertType<Internal['single']>({} as undefined);
    });

    it('handles props with function types', () => {
      const p = defineProps({
        onClick: Function as unknown as () => (e: MouseEvent) => void,
        onInput: { type: Function as unknown as () => (value: string) => void, default: () => () => {} },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal['onClick']>({} as (e: MouseEvent) => void);
      assertType<Internal['onInput']>({} as (value: string) => void);

      // @ts-expect-error cannot be undefined
      assertType<Internal['onClick']>({} as undefined);
    });

    it('handles nullable prop types', () => {
      const p = defineProps({
        nullable: [String, null] as unknown as () => string | null,
        withDefault: { type: [String, null] as unknown as () => string | null, default: null },
      });
      type P = typeof p;
      type Internal = MakeInternalProps<P>;

      assertType<Internal['nullable']>({} as string | null);
      assertType<Internal['withDefault']>({} as string | null);

      // @ts-expect-error cannot be undefined (but can be null)
      assertType<Internal['nullable']>({} as undefined);
    });
  });

  describe('Edge cases and advanced scenarios', () => {
    it('handles props with validator functions', () => {
      const p = defineProps({
        status: {
          type: String,
          default: 'pending',
          validator: (value: string) => ['pending', 'success', 'error'].includes(value),
        },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;
      type Internal = MakeInternalProps<P>;

      assertType<Public['status']>({} as string);
      assertType<Internal['status']>({} as string);
    });

    it('handles props with required flag', () => {
      const p = defineProps({
        required: { type: String, required: true },
        notRequired: { type: String, required: false },
      });
      type P = typeof p;
      type Public = MakePublicProps<P>;

      assertType<Public['required']>({} as string);
      assertType<Public['notRequired']>({} as string | undefined);
    });

    it('handles mixed required, optional, and default props', () => {
      const d = withDefaults(
        defineProps<{
          required: string;
          optional?: number;
          withDefault?: boolean;
        }>(),
        {
          withDefault: true,
        }
      );
      type D = typeof d;
      type Public = MakePublicProps<D>;
      type Internal = MakeInternalProps<D>;

      // Public: required is required, optional can be undefined, withDefault is defined
      assertType<Public['required']>({} as string);
      assertType<Public['optional']>({} as number | undefined);
      assertType<Public['withDefault']>({} as boolean);

      // Internal: all are required
      assertType<Internal['required']>({} as string);
      assertType<Internal['optional']>({} as number);
      assertType<Internal['withDefault']>({} as boolean);
    });

    it('handles generic record types', () => {
      type GenericProps = { [key: string]: any };
      type Public = MakePublicProps<GenericProps>;
      type Internal = MakeInternalProps<GenericProps>;

      assertType<Public>({} as GenericProps);
      assertType<Internal>({} as GenericProps);
    });

    it('handles intersection types', () => {
      type BaseProps = { id: number };
      type ExtendedProps = BaseProps & { name: string };
      type Public = MakePublicProps<ExtendedProps>;
      type Internal = MakeInternalProps<ExtendedProps>;

      assertType<Public>({} as ExtendedProps);
      assertType<Internal>({} as ExtendedProps);
    });

    it('differentiates between public and internal for same props', () => {
      const d = withDefaults(
        defineProps<{
          opt?: string;
        }>(),
        {}
      );
      type D = typeof d;
      type Public = MakePublicProps<D>;
      type Internal = MakeInternalProps<D>;

      // Public allows undefined
      assertType<Public['opt']>({} as string | undefined);

      // Internal requires it
      assertType<Internal['opt']>({} as string);

      // @ts-expect-error internal cannot be undefined
      assertType<Internal['opt']>({} as undefined);
    });
  });
});
