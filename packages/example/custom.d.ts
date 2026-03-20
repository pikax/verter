declare module "*.vue" {
  const component: import("vue").DefineComponent;
  export default component;
}
