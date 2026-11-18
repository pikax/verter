import { defineComponent as _defineComponent } from 'vue';
export default /*@__PURE__*/ _defineComponent({
    __name: 'App',
    setup(__props, { expose: __expose }) {
        __expose();
        function load() { try {
            JSON.parse('');
        }
        catch (err) {
            report(err);
        } }
        function report(err) { }
        const __returned__ = { load, report };
        Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true });
        return __returned__;
    }
});
