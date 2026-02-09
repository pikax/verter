<script setup lang="ts" >
import type { GameVo, ListCategory, ListParams, ListSort } from '@/services/api/index'
import { Popover, Tab, Tabs } from '@nexus/ui'
import { useQueries } from '@tanstack/vue-query'
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Icon } from '@/components/base'
import GameItem from '@/components/common/GameItem/index.vue'
import { useCommonApi } from '@/hooks'
import { getGame } from '@/services/api/index'
import { CompetitionPrize } from '@/views/index/components/CompetitionPrize'
import { CompetitionRanking } from '@/views/index/components/CompetitionRanking'
import { HotCategory, rankingTabList, SpecialTabIds } from '../../..'
import { AdBanner } from '../../AdBanner'
import { BettingRanking } from '../../BettingRanking'

const router = useRouter()

const active = ref(0)
const showTip = ref(false)
const { commonConfigData } = useCommonApi()

function goToAllGameList(id: number, name: string) {
  router.push({
    path: `/gameDetail/${id}`,
    query: { name },
  })
}

const categoryList = computed(
  () => commonConfigData.value?.category?.filter((tab) => tab.category !== SpecialTabIds.ALL) ?? []
)

// 热门 游戏查询方式 sort = 0 (按热门排序) category = 0 (全部分类)
// 剔除全部的类型选项

const filteredCategoryList = computed(() =>
  categoryList.value.filter((section) => section.category !== SpecialTabIds.ALL)
)

const gameListQueries = useQueries({
  queries: computed(() =>
    filteredCategoryList.value.map((section) => {
      let sort = 4 as ListSort
      let category = section.category as ListCategory

      // 热门特殊处理 热门分类 + 热门排序
      if (section.category === SpecialTabIds.HOT) {
        sort = HotCategory.SORT // 热门排序
        category = HotCategory.ALL as ListCategory // 全部分类
      }

      const query: ListParams = {
        page: 1,
        pageSize: 6,
        sort,
        category,
      }

      return {
        queryKey: ['game', 'list', section.category],
        queryFn: () => getGame().list(query),
        staleTime: 1000 * 30,
      }
    })
  ),
})

const gameListData = computed(() => {
  const data: Record<number, GameVo[]> = {}

  filteredCategoryList.value.forEach((section, index) => {
    const queryResult = gameListQueries.value[index]
    if (!queryResult || !queryResult.data?.list?.length) return
    data[section.category] = queryResult.data.list
  })

  return data
})
</script>

<template>
  <div>
    <AdBanner />

    <template v-for="(item, i) in categoryList" :key="item.category">
      <div v-if="i > 0 && gameListData[item.category]?.length" class="mb-[19px]">
        <div class="mb-[12px] flex items-center justify-between px-3 text-sm">
          <h2 class="font-semibold text-theme-white">{{ item.name }}</h2>
          <button
            class="click-active flex items-center text-theme-grey-800"
            @click="goToAllGameList(item.category, item.name)"
          >
            全部
            <Icon render="font" name="arrow-left" size="10" class="mx-[5px] rotate-180" />
          </button>
        </div>
        <div class="flex overflow-x-scroll px-3 gap-[7px] hide-scrollbar">
          <template v-for="game in gameListData[item.category]" :key="game.gameType">
            <div class="w-[105px] flex-shrink-0">
              <GameItem :game="game" />
            </div>
          </template>
        </div>
      </div>
    </template>

    <div class="betting-history">
      <div class="flex items-center px-3 pt-[2px] gap-[7px]">
        <h2 class="text-sm font-semibold text-theme-white">{{ rankingTabList[active].label }}</h2>

<Popover
  v-model:show="showTip"
  placement="top"
  show-arrow
  :teleport="false"
  :lock-scroll="false"
  :flip="{ padding: { top: 102 } }"
  :shift="{ padding: { top: 102 } }"
  :overlay-style="{ background: 'transparent' }"
  :offset="{ mainAxis: 8, crossAxis: 88 }"
>
          <template #reference>
            <div class="text-theme-grey-800">
              <Icon render="font" name="tip" size="14" />
            </div>
          </template>
          <div class="w-[228px] bg-theme-white px-3 py-4 text-[13px] leading-5 text-theme-black">
            为防止过快刷屏，“最近投注”与“大额输赢”只显示部分注单
          </div>
        </Popover>
      </div>

      <Tabs v-model:active="active" background="transparent" auto-height class="mt-[12px]">
        <Tab v-for="(tab, index) in rankingTabList" :key="tab.key">
          <template #title>
            <button
              class="w-full flex-1 rounded-[30px] py-[7.5px] text-[14px] transition-all duration-300 ease-in-out"
              :class="
                active === index ? 'bg-[#FFFFFF0F] font-bold text-theme-white' : 'text-[#C0C0C0]'
              "
            >
              {{ tab.label }}
            </button>
          </template>

          <div class="px-[12px] pb-2 text-sm">
            <template v-if="index === 2">
              <CompetitionPrize direction="row" class="mb-[5px] mt-[13px]" />
              <CompetitionRanking />
            </template>

            <BettingRanking v-else />
          </div>
        </Tab>
      </Tabs>
    </div>
  </div>
</template>

<style lang="scss" scoped>
:deep(.ns-tabs) {
  .ns-tabs__nav {
    display: flex;
    padding: 3px;
    margin: 0 12px;
    background-color: #ffffff0f;
    border-radius: 1.875rem;
  }

  .ns-navbar__item {
    flex: 1 1 0%;
  }
}
</style>
