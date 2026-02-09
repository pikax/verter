import { createApp } from 'vue'
import App from './App.vue'
import './style.css'
import { registerVueLanguage } from './editor/vueLanguage'
import { registerLanguages } from './editor/languageConfigs'

// Register languages for Monaco
registerVueLanguage()
registerLanguages()

const app = createApp(App)
app.mount('#app')
