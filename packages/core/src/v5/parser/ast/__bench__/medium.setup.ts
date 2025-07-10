/**
 * Import the necessary Composition API functions
 */
import {
  ref,
  reactive,
  computed,
  watch,
  onMounted,
  onBeforeUnmount,
} from "vue";

/**
 * Example interface for typed items
 */
interface LargeItem {
  title: string;
  description: string;
}

/**
 * 1) Create a large array of items
 */
const largeArray = ref<LargeItem[]>([]);
for (let i = 1; i <= 50; i++) {
  largeArray.value.push({
    title: `Item ${i}`,
    description: `This is the description for item ${i}.`,
  });
}

/**
 * 2) Some reactive references and watchers
 */

// Computed to get current count of largeArray
const itemCount = computed(() => largeArray.value.length);

// Ref to store if the count is even
const itemCountEven = ref(false);

// Watch itemCount changes and update itemCountEven
watch(itemCount, (newVal) => {
  itemCountEven.value = newVal % 2 === 0;
});

// anotherRef increments whenever itemCountEven becomes true
const anotherRef = ref(0);
watch(itemCountEven, (newVal) => {
  if (newVal) {
    anotherRef.value++;
  }
});

// finalRef depends on changes of anotherRef
const finalRef = ref("");
watch(anotherRef, (newVal) => {
  finalRef.value = `anotherRef has been triggered ${newVal} times.`;
});

/**
 * 3) Watchers that watch watchers
 */
const secondWatcherRef = ref(0);
watch(finalRef, (newVal) => {
  // store the length of finalRef in secondWatcherRef
  secondWatcherRef.value = newVal.length;
});

const thirdWatcherRef = ref("");
watch(secondWatcherRef, (newVal) => {
  // build a string from secondWatcherRef
  thirdWatcherRef.value = `finalRef length is: ${newVal}`;
});

/**
 * 4) Computed properties that combine or derive data
 */
const combinedTitle = computed(() => {
  return largeArray.value.map((item) => item.title).join(" | ");
});

const combinedCount = computed(() => {
  return `We have ${itemCount.value} total items in the array.`;
});

const isCountEven = computed(() => itemCountEven.value);

/**
 * 5) Reactive object and methods
 */
const userInfo = reactive({
  name: "TestUser",
  loggedIn: false,
});

function toggleLogin() {
  userInfo.loggedIn = !userInfo.loggedIn;
}

function renameUser(newName: string) {
  userInfo.name = newName;
}

/**
 * 6) Another chain of watchers to illustrate “watchers on watchers”
 */
const chainRef = ref(0);
const chainRefWatcher = ref(0);
const chainRefFinal = ref("");

function incrementChainRef() {
  chainRef.value++;
}

// Watch chainRef => update chainRefWatcher
watch(chainRef, (newVal) => {
  chainRefWatcher.value = newVal * 10; // multiply by 10
});

// Watch chainRefWatcher => set chainRefFinal
watch(chainRefWatcher, (newVal) => {
  chainRefFinal.value = `chainRef multiplied by 10 is ${newVal}.`;
});

/**
 * 7) Generate some random data to fill the template
 */
interface RandomData {
  number: number;
  text: string;
}

const randomData = ref<RandomData[]>([]);
for (let i = 0; i < 20; i++) {
  randomData.value.push({
    number: Math.floor(Math.random() * 1000),
    text: `Random text #${i + 1}`,
  });
}

/**
 * Lifecycle hooks
 */
onMounted(() => {
  console.log("Huge component mounted.");
});

onBeforeUnmount(() => {
  console.log("Huge component about to be unmounted.");
});
