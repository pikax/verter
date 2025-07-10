export default {
  data() {
    return {
      largeArray: [],
      userInfo: {
        name: "DefaultUser",
        age: 25,
        loggedIn: false,
        preferences: {
          theme: "light",
          notifications: true,
        },
      },
      counter: 0,
      counterHistory: [],
      chainWatcherValue: 0,
      chainResult: "",
      randomData: [],
    };
  },
  computed: {
    totalValue() {
      return this.largeArray.reduce((sum, item) => sum + item.value, 0);
    },
    averageValue() {
      return this.largeArray.length
        ? this.totalValue / this.largeArray.length
        : 0;
    },
    highValueItems() {
      return this.largeArray.filter((item) => item.value > 500);
    },
    sortedRandomData() {
      return [...this.randomData].sort((a, b) => a.number - b.number);
    },
  },
  watch: {
    highValueItems(newVal) {
      console.log("High-value items updated:", newVal);
    },
    userInfo: {
      handler(newVal) {
        console.log("User info updated:", newVal);
      },
      deep: true,
    },
    counter(newVal) {
      this.counterHistory.push({ timestamp: Date.now(), value: newVal });
    },
    chainWatcherValue(newVal) {
      this.chainResult = `Value doubled: ${newVal * 2}`;
    },
    chainResult(newVal) {
      console.log("Chain result updated:", newVal);
    },
  },
  methods: {
    updateUserInfo(newInfo) {
      Object.assign(this.userInfo, newInfo);
    },
    incrementCounter() {
      this.counter++;
    },
    addItem(name, description, value) {
      const newItem = {
        id: this.largeArray.length + 1,
        name,
        description,
        value,
      };
      this.largeArray.push(newItem);
    },
    removeItemById(id) {
      this.largeArray = this.largeArray.filter((item) => item.id !== id);
    },
    incrementChainWatcher() {
      this.chainWatcherValue++;
    },
  },
  created() {
    for (let i = 1; i <= 10000; i++) {
      this.largeArray.push({
        id: i,
        name: `Item ${i}`,
        description: `This is the description for item ${i}.`,
        value: Math.floor(Math.random() * 1000),
      });
    }

    for (let i = 0; i < 10000; i++) {
      this.randomData.push({
        id: i,
        number: Math.random() * 1000,
        text: `Random text for item ${i}`,
      });
    }
  },
  mounted() {
    console.log(
      "Component mounted. Initial high-value items:",
      this.highValueItems
    );
  },
  beforeDestroy() {
    console.log("Component about to be destroyed.");
  },
};
